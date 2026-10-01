use super::{
    ApiError, ApiState, InvocationContext, InvocationOrigin, InvocationSource, LocalNode, Principal,
};
use crate::auth::surface::{
    DOCUMENT_HEADER, SurfaceAccess, SurfaceAuthorization, require_owner, validate_document,
};
use axum::http::HeaderMap;
use babble_judgment::JudgmentProvider;
use babble_runtime::{SurfaceLifecycle, SurfaceSessionId};
use babble_types::{Canonical, IdentityId};

const HOST_DOCUMENT_HEADER: &str = "x-babble-host-document";

pub(super) fn require_header(
    headers: &HeaderMap,
    origin: &InvocationSource,
) -> Result<(), ApiError> {
    let (name, other, document) = match origin {
        InvocationSource::HostAction { document_id } => {
            (HOST_DOCUMENT_HEADER, DOCUMENT_HEADER, document_id)
        }
        InvocationSource::Surface { document_id, .. } => {
            (DOCUMENT_HEADER, HOST_DOCUMENT_HEADER, document_id)
        }
    };
    if headers.contains_key(other) {
        return Err(ApiError::forbidden());
    }
    let mut values = headers.get_all(name).iter();
    let value = values.next().ok_or_else(ApiError::forbidden)?;
    if values.next().is_some() || value.to_str().ok() != Some(document.as_str()) {
        return Err(ApiError::forbidden());
    }
    validate_document(document)
}

pub(super) fn current<P: JudgmentProvider>(
    state: &ApiState<P>,
    node: &LocalNode<P>,
    principal: &Principal,
    object: &str,
    source: &InvocationSource,
) -> Result<InvocationContext, ApiError> {
    let principal = state
        .auth
        .with_store(|store| store.authenticate_hash(&principal.account_session))?;
    let object_id = crate::routes::object_id(object.to_owned())?;
    let object = node
        .object(&object_id)
        .ok_or_else(|| ApiError::not_found("invocation Object not found"))?;
    let origin = match source {
        InvocationSource::HostAction { document_id } => {
            if object.author.as_str() != principal.identity_id {
                return Err(ApiError::forbidden());
            }
            state.auth.with_store(|store| {
                store.require_host_document(
                    &principal,
                    document_id,
                    object_id.as_str(),
                    node.invocation_epoch().as_str(),
                )
            })?;
            InvocationOrigin::HostAction {
                document_id: document_id.clone(),
            }
        }
        InvocationSource::Surface {
            session_id,
            document_id,
        } => {
            require_owner(
                &state.auth,
                node,
                &principal,
                &SurfaceAuthorization {
                    session: session_id.clone(),
                    object: Some(object_id.to_string()),
                    document: Some(document_id.clone()),
                    access: SurfaceAccess::Execute,
                },
            )?;
            let session = node.surface_session(&SurfaceSessionId::new(session_id)?)?;
            if session.lifecycle != SurfaceLifecycle::Active
                || session.plan.surface.role == babble_object::SurfaceRole::Background
            {
                return Err(ApiError::forbidden());
            }
            let surface = &session.plan.surface;
            let resource_digest = session
                .plan
                .bundle_verification
                .as_ref()
                .map(|b| b.manifest_hash.clone())
                .or_else(|| surface.integrity.clone())
                .ok_or_else(ApiError::forbidden)?;
            let role = serde_json::to_value(&surface.role)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .as_str()
                .ok_or_else(ApiError::forbidden)?
                .to_ascii_lowercase();
            InvocationOrigin::Surface {
                session_id: session_id.clone(),
                document_id: document_id.clone(),
                role,
                entry: surface.entry.clone(),
                resource_digest,
            }
        }
    };
    Ok(InvocationContext {
        actor: IdentityId::new_unchecked(principal.identity_id),
        login_id: principal.account_session,
        object_id,
        object_version: object.canonical_hash()?,
        origin,
        policy_revision: node.invocation_policy_revision()?,
        context_epoch: node.invocation_epoch(),
    })
}
