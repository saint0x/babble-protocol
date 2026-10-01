use super::{Auth, Ordering, Principal, random_token};
use crate::{
    ApiState,
    error::ApiError,
    routes::{lock_node, object_id},
    schema::StartSurfaceSessionRequest,
};
use babble_judgment::JudgmentProvider;
use babble_node::LocalNode;
use babble_runtime::{SurfaceLifecycle, SurfaceSession, SurfaceSessionId};
use babble_types::IdentityId;

pub(crate) fn authorize_bundle<P: JudgmentProvider>(
    auth: &Auth,
    node: &LocalNode<P>,
    principal: &Principal,
    session: &SurfaceSessionId,
    object: &babble_types::ObjectId,
    allow_suspended: bool,
) -> Result<(), ApiError> {
    let current = auth.with_store(|store| store.authenticate_hash(&principal.account_session))?;
    if current.identity_id != principal.identity_id {
        return Err(ApiError::forbidden());
    }
    require_owner(
        auth,
        node,
        &current,
        &SurfaceAuthorization {
            session: session.to_string(),
            object: Some(object.to_string()),
            document: None,
            access: if allow_suspended {
                SurfaceAccess::Manage
            } else {
                SurfaceAccess::Execute
            },
        },
    )
}

#[derive(Clone, Copy)]
pub(crate) enum SurfaceAccess {
    Execute,
    Renew,
    Manage,
    Evict,
    Inspect,
}

#[derive(Clone)]
pub(crate) struct SurfaceAuthorization {
    pub session: String,
    pub object: Option<String>,
    pub document: Option<String>,
    pub access: SurfaceAccess,
}

#[derive(Clone)]
pub(crate) struct ExecutionAuthorization {
    pub principal: Principal,
    pub surface: Option<SurfaceAuthorization>,
}

tokio::task_local! {
    pub(crate) static EXECUTION: ExecutionAuthorization;
}

pub(crate) fn require_owner<P: JudgmentProvider>(
    auth: &Auth,
    node: &LocalNode<P>,
    principal: &Principal,
    requirement: &SurfaceAuthorization,
) -> Result<(), ApiError> {
    if !auth.with_store(|store| {
        store.owns_surface(
            &requirement.session,
            principal,
            requirement.object.as_deref(),
            matches!(
                requirement.access,
                SurfaceAccess::Inspect | SurfaceAccess::Evict
            ),
        )
    })? {
        return Err(ApiError::forbidden());
    }
    let session = node.surface_session(&SurfaceSessionId::new(&requirement.session)?)?;
    if !matches!(requirement.access, SurfaceAccess::Inspect | SurfaceAccess::Evict) {
        node.require_moderation_execution(&session.plan.object_id)?;
    }
    if requirement
        .object
        .as_deref()
        .is_some_and(|id| id != session.plan.object_id.as_str())
    {
        return Err(ApiError::forbidden());
    }
    let allowed = match requirement.access {
        SurfaceAccess::Execute | SurfaceAccess::Renew => matches!(
            session.lifecycle,
            SurfaceLifecycle::Prefetched | SurfaceLifecycle::Warm | SurfaceLifecycle::Active
        ),
        SurfaceAccess::Manage => session.lifecycle != SurfaceLifecycle::Evicted,
        SurfaceAccess::Evict | SurfaceAccess::Inspect => true,
    };
    if !allowed {
        return Err(ApiError::forbidden());
    }
    if let Some(document) = &requirement.document {
        if !auth.with_store(|store| {
            store.matches_surface_document(&requirement.session, principal, document)
        })? {
            return Err(ApiError::forbidden());
        }
    }
    Ok(())
}

pub(crate) const DOCUMENT_HEADER: &str = "x-babble-surface-document";

pub(crate) fn validate_document(document: &str) -> Result<(), ApiError> {
    if document.len() != 36
        || !document.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
    {
        return Err(ApiError::bad_request(
            "document_id must be a lowercase canonical UUID",
        ));
    }
    Ok(())
}

pub(crate) fn bind_surface_document<P: JudgmentProvider>(
    state: &ApiState<P>,
    id: SurfaceSessionId,
    document: String,
    principal: &Principal,
) -> Result<crate::schema::SurfaceDocumentResponse, ApiError> {
    validate_document(&document)?;
    let node = lock_node(state)?;
    let current = state
        .auth
        .with_store(|store| store.authenticate_hash(&principal.account_session))?;
    require_owner(
        &state.auth,
        &node,
        &current,
        &SurfaceAuthorization {
            session: id.to_string(),
            object: None,
            document: None,
            access: SurfaceAccess::Execute,
        },
    )?;
    state
        .auth
        .with_store(|store| store.bind_surface_document(id.as_str(), &current, &document))?;
    Ok(crate::schema::SurfaceDocumentResponse {
        session_id: id,
        document_id: document,
    })
}

// Called only after acquiring the execution mutex. Admission is not authority
// to execute after a queued request's credential or Surface has been invalidated.
pub(crate) fn check_execution<P: JudgmentProvider>(
    state: &ApiState<P>,
    node: &mut LocalNode<P>,
) -> Result<(), ApiError> {
    node.reconcile_surface_permissions()?;
    EXECUTION
        .try_with(|context| {
            let current = state
                .auth
                .with_store(|store| store.authenticate_hash(&context.principal.account_session))?;
            if current.identity_id != context.principal.identity_id {
                return Err(ApiError::forbidden());
            }
            if let Some(requirement) = &context.surface {
                require_owner(&state.auth, node, &current, requirement)?;
            }
            Ok(())
        })
        .unwrap_or(Ok(()))
}

pub(super) fn cleanup_locked<P: JudgmentProvider>(
    auth: &Auth,
    node: &mut LocalNode<P>,
    after: &str,
) -> Result<Option<String>, ApiError> {
    node.reconcile_surface_permissions()?;
    crate::invocations::cleanup_host_contexts(auth, node)?;
    let sessions = auth.with_store(|store| store.surface_cleanup_batch(after))?;
    let next = if sessions.len() == super::store::CLEANUP_BATCH {
        sessions.last().map(|(id, _)| id.clone())
    } else {
        None
    };
    for (id, abandoned) in sessions {
        let terminal = node
            .surface_session(&SurfaceSessionId::new(&id)?)
            .is_ok_and(|session| session.lifecycle == SurfaceLifecycle::Evicted);
        if abandoned || terminal {
            retire_runtime(auth, node, &id)?;
        }
    }
    Ok(next)
}

pub(super) fn cleanup_origin_locked<P: JudgmentProvider>(
    auth: &Auth,
    node: &mut LocalNode<P>,
    account_session: &str,
) -> Result<(), ApiError> {
    crate::invocations::cleanup_host_contexts(auth, node)?;
    loop {
        let sessions = auth.with_store(|store| store.origin_surface_batch(account_session))?;
        if sessions.is_empty() {
            return Ok(());
        }
        for id in sessions {
            retire_runtime(auth, node, &id)?;
        }
    }
}

fn retire_runtime<P: JudgmentProvider>(
    auth: &Auth,
    node: &mut LocalNode<P>,
    id: &str,
) -> Result<(), ApiError> {
    let session_id = SurfaceSessionId::new(id)?;
    match node.surface_session(&session_id) {
        Ok(session) if session.lifecycle != SurfaceLifecycle::Evicted => {
            node.transition_surface_session(
                &session_id,
                SurfaceLifecycle::Evicted,
                "Surface host lease or account session expired or revoked",
            )?;
        }
        Ok(_) | Err(babble_types::Error::NotFound(_)) => {}
        Err(error) => return Err(error.into()),
    }
    auth.with_store(|store| store.retire_surface(id))
}

pub(crate) fn heartbeat_surface<P: JudgmentProvider>(
    state: &ApiState<P>,
    id: SurfaceSessionId,
    principal: &Principal,
) -> Result<crate::schema::SurfaceLeaseResponse, ApiError> {
    let node = lock_node(state)?;
    require_owner(
        &state.auth,
        &node,
        principal,
        &SurfaceAuthorization {
            session: id.to_string(),
            object: None,
            document: None,
            access: SurfaceAccess::Renew,
        },
    )?;
    let (now, deadline) = state
        .auth
        .with_store(|store| store.renew_surface(id.as_str(), principal))?;
    let expires_at =
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(deadline) * 1_000_000)
            .map_err(|_| ApiError::internal("invalid Surface lease deadline"))?
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| ApiError::internal("invalid Surface lease deadline"))?;
    let ttl_ms = (deadline - now) as u64;
    Ok(crate::schema::SurfaceLeaseResponse {
        lease: crate::schema::SurfaceLease {
            session_id: id,
            expires_at,
            ttl_ms,
            renew_after_ms: 15_000.min((ttl_ms / 3).max(1)),
        },
    })
}

pub(super) fn start_cleanup<P: JudgmentProvider + Send + Sync + 'static>(state: &ApiState<P>) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    if state.auth.cleanup_started.swap(true, Ordering::AcqRel) {
        return;
    }
    let node = std::sync::Arc::downgrade(&state.node);
    let auth = std::sync::Arc::downgrade(&state.auth);
    let gateway = state.gateway.as_ref().map(std::sync::Arc::downgrade);
    runtime.spawn(async move {
        let mut cursor = String::new();
        loop {
            let (Some(node), Some(auth)) = (node.upgrade(), auth.upgrade()) else {
                break;
            };
            let after = cursor.clone();
            let gateway = gateway.as_ref().and_then(std::sync::Weak::upgrade);
            let result = tokio::task::spawn_blocking(move || {
                let mut node = node
                    .lock()
                    .map_err(|_| ApiError::internal("local node lock is poisoned"))?;
                let next = cleanup_locked(&auth, &mut node, &after)?;
                if let Some(gateway) = gateway {
                    gateway.prune(&auth, &node)?;
                }
                Ok::<_, ApiError>(next)
            })
            .await;
            match result {
                Ok(Ok(Some(next))) => {
                    cursor = next;
                    tokio::task::yield_now().await;
                    continue;
                }
                Ok(Ok(None)) => cursor.clear(),
                _ => eprintln!("Surface account-session cleanup failed; will retry"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

pub(crate) fn start_surface<P: JudgmentProvider>(
    state: &ApiState<P>,
    input: StartSurfaceSessionRequest,
    principal: &Principal,
) -> Result<SurfaceSession, ApiError> {
    let id = match input.session_id {
        Some(id) => {
            id.validate()?;
            id
        }
        None => SurfaceSessionId::new(format!("surf_{}", random_token()?))?,
    };
    let object = object_id(input.object_id)?;
    // Serialize the native session and its ownership decision under the node lock.
    // An existing native session must never be claimed by a later HTTP request.
    let mut node = lock_node(state)?;
    if let Ok(mut session) = node.surface_session(&id) {
        let owned = state.auth.with_store(|store| {
            store.owns_surface(id.as_str(), principal, Some(object.as_str()), false)
        })?;
        if !owned
            || session.plan.object_id != object
            || session.plan.surface.role != input.role
            || session.lifecycle == SurfaceLifecycle::Evicted
        {
            return Err(ApiError::forbidden());
        }
        if session.plan.bundle_verification.is_some() {
            session.plan.verified_mount = Some(
                state
                    .gateway
                    .as_ref()
                    .ok_or_else(|| ApiError::conflict("bundle gateway unavailable"))?
                    .descriptor(&id, &state.auth, &node)?,
            );
        }
        return Ok(session);
    }
    // The runtime's captured grants and lifecycle do not survive a restart.
    // Never reconstruct an old document's authority from newly approved grants.
    if state
        .auth
        .with_store(|store| store.has_surface_document(id.as_str()))?
    {
        state
            .auth
            .with_store(|store| store.retire_surface(id.as_str()))?;
        return Err(ApiError::forbidden());
    }
    let receipt = if state.gateway.is_some()
        && node.object(&object).is_some_and(|object| {
            object
                .surfaces
                .iter()
                .any(|surface| surface.role == input.role && surface.bundle.is_some())
        }) {
        Some(node.verify_surface_bundle(&object, input.role.clone())?)
    } else {
        None
    };
    if let Some(gateway) = state.gateway.as_ref() {
        gateway.prune(&state.auth, &node)?;
        if let Some(receipt) = receipt.as_ref() {
            gateway.check_capacity(receipt)?;
        }
    }
    let created = state
        .auth
        .with_store(|store| store.reserve_surface(id.as_str(), principal, object.as_str()))?;
    let identity = IdentityId::new_unchecked(principal.identity_id.clone());
    let result = if let Some(receipt) = receipt.as_ref() {
        node.start_verified_surface_session_for_identity(
            &object,
            input.role,
            id.clone(),
            &identity,
            receipt,
        )
    } else {
        node.start_surface_session_for_identity(&object, input.role, id.clone(), &identity)
    };
    if result.is_err() && created {
        state
            .auth
            .with_store(|store| store.remove_surface(id.as_str()))?;
    }
    let mut session = result.map_err(ApiError::from)?;
    if let Some(receipt) = receipt {
        let mounted = state
            .gateway
            .as_ref()
            .expect("receipt requires gateway")
            .allocate(&session, receipt, principal);
        match mounted {
            Ok(descriptor) => session.plan.verified_mount = Some(descriptor),
            Err(error) => {
                node.transition_surface_session(
                    &id,
                    SurfaceLifecycle::Evicted,
                    "verified bundle mount failed",
                )?;
                state
                    .auth
                    .with_store(|store| store.retire_surface(id.as_str()))?;
                return Err(error);
            }
        }
    }
    Ok(session)
}
