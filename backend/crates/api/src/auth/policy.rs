use super::Principal;
use super::surface::{
    self, EXECUTION, ExecutionAuthorization, SurfaceAccess, SurfaceAuthorization,
};
use crate::{
    ApiState,
    error::ApiError,
    routes::{lock_node, object_id},
};
use axum::{
    body::{Body, to_bytes},
    extract::{MatchedPath, Query, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use babel_judgment::JudgmentProvider;
use babel_rpc::RpcRequestEnvelope;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Public,
    User,
    Author,
    Grant,
    Revoke,
    ObjectOwner,
    Sync,
    StartSurface,
    Surface,
    Operator,
    Denied,
}

// Each transport operation is deliberately listed. Adding a handler or catalog
// method without an explicit policy leaves it inaccessible over HTTP.
fn rest_access(method: &str, route: &str) -> Access {
    use Access::*;
    match (method, route) {
        ("GET", "/moderation/access" | "/moderation/reports" | "/moderation/reports/{id}") => User,
        ("POST", "/moderation/reports" | "/moderation/reports/{id}/decisions" | "/moderation/reports/{id}/appeals") => User,
        ("GET" | "HEAD", "/objects/{id}/media/{hash}") => Public,
        (
            "GET",
            "/health"
            | "/rpc/catalog"
            | "/identities/{id}"
            | "/identities/{id}/objects"
            | "/objects/{id}"
            | "/objects/{id}/judgments"
            | "/objects/{id}/capabilities"
            | "/objects/{id}/reactions"
            | "/objects/{id}/quotes"
            | "/objects/{id}/reactions/actors/{actor}"
            | "/graph/edges/{id}"
            | "/graph/objects/{id}/incoming"
            | "/graph/objects/{id}/outgoing"
            | "/graph/objects/{id}/evidence"
            | "/judgments/definitions"
            | "/judgments/providers"
            | "/judgments/{id}"
            | "/search/objects"
            | "/lenses"
            | "/capabilities"
            | "/runtime/surfaces/blobs/{hash}"
            | "/media/blobs/{hash}",
        ) => Public,
        (
            "POST",
            "/graph/objects/{id}/traverse" | "/discovery/candidates" | "/runtime/surfaces/prepare",
        ) => Public,
        (
            "GET",
            "/observability"
            | "/events"
            | "/events/{id}"
            | "/consensus/checkpoints/{id}"
            | "/runtime/surfaces/health",
        ) => Operator,
        (
            "POST",
            "/events/bundle"
            | "/events/import"
            | "/consensus/checkpoints"
            | "/consensus/checkpoints/preview",
        ) => Operator,
        ("POST", "/identities") => Denied,
        (
            "POST",
            "/objects"
            | "/objects/text"
            | "/objects/media"
            | "/objects/forks"
            | "/objects/remixes"
            | "/graph/edges"
            | "/graph/relationships/infer"
            | "/realtime/sessions"
            | "/realtime/messages",
        ) => Author,
        ("DELETE", "/realtime/sessions/{id}") => Author,
        ("POST", "/capabilities/grants") => Grant,
        ("POST", "/capabilities/revocations") => Revoke,
        ("POST", "/realtime/rooms") => ObjectOwner,
        ("POST", "/identities/{id}/keys/rotate" | "/judgments/object/{id}" | "/media/blobs") => {
            User
        }
        ("GET", "/realtime/rooms/{id}") => User,
        ("PUT" | "DELETE", "/invocations/v1/documents/{document_id}") => User,
        (
            "POST",
            "/invocations/v1/prepare"
            | "/invocations/v1/recover"
            | "/invocations/v1/{id}/decision"
            | "/invocations/v1/{id}/execute"
            | "/invocations/v1/{id}/cancel"
            | "/invocations/v1/browser/prepare"
            | "/invocations/v1/browser/{id}/decision"
            | "/invocations/v1/browser/{id}/dispatch"
            | "/invocations/v1/browser/{id}/ack"
            | "/invocations/v1/browser/{id}/cancel",
        ) => User,
        ("GET", "/invocations/v1/{id}/status" | "/invocations/v1/browser/{id}/status") => User,
        ("GET", "/social/following" | "/social/following/{id}" | "/feed/following") => User,
        ("PUT", "/social/following/{id}") => User,
        ("GET", "/social/safety" | "/social/safety/{id}") => User,
        ("PUT", "/social/safety/{id}") => User,
        ("GET" | "PUT", "/objects/{id}/reactions/mine") => User,
        ("POST" | "GET", "/personalization/sync/envelopes") => Sync,
        ("GET" | "DELETE", "/personalization/sync/envelopes/{hash}") => Sync,
        ("POST", "/runtime/surfaces/sessions") => StartSurface,
        ("PUT", "/runtime/surfaces/sessions/{id}/document") => Surface,
        ("GET", "/runtime/surfaces/sessions/{id}" | "/runtime/surfaces/sessions/{id}/state") => {
            Surface
        }
        (
            "POST",
            "/runtime/surfaces/sessions/{id}/lifecycle"
            | "/runtime/surfaces/sessions/{id}/heartbeat"
            | "/runtime/surfaces/sessions/{id}/budget"
            | "/runtime/surfaces/sessions/{id}/schedule"
            | "/runtime/surfaces/sessions/{id}/schedule/apply"
            | "/runtime/surfaces/sessions/{id}/state/checkpoint",
        ) => Surface,
        _ => Denied,
    }
}

fn rpc_access(method: &str) -> Access {
    if crate::invocations::social_method(method) {
        return Access::Author;
    }
    use Access::*;
    match method {
        "babel.object.get.v1"
        | "babel.media.blob.get.v1"
        | "babel.graph.evidence.v1"
        | "babel.graph.traverse.v1"
        | "babel.social.replies.list.v1"
        | "babel.social.quotes.list.v1"
        | "babel.social.reactions.summary.v1"
        | "babel.social.reactions.record.v1"
        | "babel.judgment.definitions.list.v1"
        | "babel.judgment.providers.list.v1"
        | "babel.judgment.object.list.v1"
        | "babel.search.objects.v1"
        | "babel.lenses.list.v1"
        | "babel.discovery.candidates.v1"
        | "babel.capabilities.list.v1"
        | "babel.capabilities.inspect.v1"
        | "babel.runtime.surface.prepare.v1" => Public,
        "babel.observability.snapshot.v1"
        | "babel.events.list.v1"
        | "babel.events.bundle.v1"
        | "babel.events.import.v1"
        | "babel.consensus.checkpoint.preview.v1"
        | "babel.consensus.checkpoint.publish.v1"
        | "babel.runtime.surface.health.v1" => Operator,
        "babel.object.publish_text.v1"
        | "babel.object.publish.v1"
        | "babel.object.publish_media.v1"
        | "babel.object.fork.v1"
        | "babel.object.remix.v1"
        | "babel.graph.edge.publish.v1"
        | "babel.graph.relationship.infer.v1"
        | "babel.social.follow.v1"
        | "babel.social.unfollow.v1"
        | "babel.social.share.v1"
        | "babel.social.reply.v1"
        | "babel.realtime.session.start.v1"
        | "babel.realtime.session.leave.v1"
        | "babel.realtime.message.publish.v1" => Author,
        "babel.capabilities.grant.v1" => Grant,
        "babel.capabilities.revoke.v1" => Revoke,
        "babel.realtime.room.define.v1" => ObjectOwner,
        "babel.identity.current.v1"
        | "babel.social.reactions.mine.v1"
        | "babel.social.reactions.set.v1"
        | "babel.media.blob.put.v1"
        | "babel.judgment.object.evaluate.v1"
        | "babel.ai.judge.v1"
        | "babel.ai.generate.v1"
        | "babel.ai.embed.v1"
        | "babel.ai.transcribe.v1"
        | "babel.storage.local.get.v1"
        | "babel.storage.local.set.v1"
        | "babel.storage.local.delete.v1"
        | "babel.storage.local.list.v1"
        | "babel.storage.object.get.v1"
        | "babel.storage.object.set.v1"
        | "babel.storage.object.delete.v1"
        | "babel.storage.object.list.v1"
        | "babel.network.fetch.v1"
        | "babel.payments.checkout.v1"
        | "babel.notifications.request.v1"
        | "babel.media.camera.request.v1"
        | "babel.media.microphone.request.v1"
        | "babel.clipboard.write.v1"
        | "babel.fullscreen.enter.v1"
        | "babel.clipboard.write.v2"
        | "babel.fullscreen.enter.v2" => User,
        "babel.personalization.sync.put.v1"
        | "babel.personalization.sync.list.v1"
        | "babel.personalization.sync.get.v1"
        | "babel.personalization.sync.delete.v1" => Sync,
        "babel.runtime.surface.session.start.v1" => StartSurface,
        "babel.runtime.surface.session.get.v1"
        | "babel.runtime.surface.session.heartbeat.v1"
        | "babel.runtime.surface.session.transition.v1"
        | "babel.runtime.surface.session.budget.v1"
        | "babel.runtime.surface.session.schedule.v1"
        | "babel.runtime.surface.session.apply_schedule.v1"
        | "babel.runtime.surface.session.state.checkpoint.v1"
        | "babel.runtime.surface.session.state.get.v1" => Surface,
        _ => Denied,
    }
}

fn surface_method(method: &str) -> bool {
    if crate::invocations::social_method(method) {
        return true;
    }
    matches!(
        method,
        "babel.identity.current.v1"
            | "babel.ai.judge.v1"
            | "babel.ai.generate.v1"
            | "babel.ai.embed.v1"
            | "babel.ai.transcribe.v1"
            | "babel.social.follow.v1"
            | "babel.social.unfollow.v1"
            | "babel.social.share.v1"
            | "babel.social.reply.v1"
            | "babel.storage.local.get.v1"
            | "babel.storage.local.set.v1"
            | "babel.storage.local.delete.v1"
            | "babel.storage.local.list.v1"
            | "babel.storage.object.get.v1"
            | "babel.storage.object.set.v1"
            | "babel.storage.object.delete.v1"
            | "babel.storage.object.list.v1"
            | "babel.network.fetch.v1"
            | "babel.payments.checkout.v1"
            | "babel.notifications.request.v1"
            | "babel.media.camera.request.v1"
            | "babel.media.microphone.request.v1"
            | "babel.clipboard.write.v1"
            | "babel.fullscreen.enter.v1"
            | "babel.clipboard.write.v2"
            | "babel.fullscreen.enter.v2"
            | "babel.realtime.session.start.v1"
            | "babel.realtime.session.leave.v1"
            | "babel.realtime.message.publish.v1"
            | "babel.runtime.surface.session.get.v1"
            | "babel.runtime.surface.session.state.get.v1"
    )
}

pub(crate) async fn authorize<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    request: Request,
    next: Next,
) -> Response {
    surface::start_cleanup(&state);
    match checked_request(&state, request).await {
        Ok(request) => {
            let context = request
                .extensions()
                .get::<ExecutionAuthorization>()
                .cloned();
            let mut response = match context {
                Some(context) => EXECUTION.scope(context, next.run(request)).await,
                None => next.run(request).await,
            };
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(error) => {
            let mut response = error.into_response();
            response.headers_mut().insert(axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"));
            response
        },
    }
}

async fn checked_request<P: JudgmentProvider>(
    state: &ApiState<P>,
    mut request: Request,
) -> Result<Request, ApiError> {
    request.extensions_mut().insert(super::HttpBoundary);
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_default();
    let is_rpc = request.method() == "POST" && route == "/rpc";
    if !is_rpc
        && !route.starts_with("/invocations/v1/")
        && request.headers().contains_key(surface::DOCUMENT_HEADER)
    {
        return Err(ApiError::bad_request(
            "Surface document header requires an object-bound RPC session",
        ));
    }
    let access = rest_access(request.method().as_str(), &route);
    if !is_rpc && access == Access::Public {
        if request
            .headers()
            .contains_key(axum::http::header::AUTHORIZATION)
        {
            let principal = state.auth.principal(request.headers())?;
            request.extensions_mut().insert(ExecutionAuthorization {
                principal: principal.clone(),
                surface: None,
            });
            request.extensions_mut().insert(principal);
        }
        return Ok(request);
    }
    if !is_rpc && access == Access::Denied {
        return Err(ApiError::forbidden());
    }
    if !is_rpc && access == Access::Operator {
        state.auth.operator(request.headers())?;
        return Ok(request);
    }
    let (mut parts, body) = request.into_parts();
    let bytes = to_bytes(body, crate::execution::MAX_BODY_BYTES)
        .await
        .map_err(|_| ApiError::bad_request("request body exceeds limit"))?;
    if is_rpc {
        let mut rpc: RpcRequestEnvelope = serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::bad_request("invalid RPC request"))?;
        let mut documents = parts.headers.get_all(surface::DOCUMENT_HEADER).iter();
        let document = documents.next();
        if documents.next().is_some() {
            return Err(ApiError::bad_request("duplicate Surface document header"));
        }
        let document =
            if rpc.binding.object_id.is_some() && rpc.binding.surface_session_id.is_some() {
                let document = document
                    .ok_or_else(ApiError::forbidden)?
                    .to_str()
                    .map_err(|_| ApiError::bad_request("invalid Surface document header"))?;
                surface::validate_document(document)?;
                Some(document.to_owned())
            } else {
                if document.is_some() {
                    return Err(ApiError::bad_request(
                        "Surface document header requires an object-bound RPC session",
                    ));
                }
                None
            };
        let access = rpc_access(rpc.method.as_str());
        if access == Access::Denied {
            return Err(ApiError::forbidden());
        }
        if rpc.binding.object_id.is_some()
            && access != Access::Public
            && !surface_method(rpc.method.as_str())
        {
            return Err(ApiError::forbidden());
        }
        if access == Access::Operator {
            state.auth.operator(&parts.headers)?;
        } else if access != Access::Public
            || rpc.binding.surface_session_id.is_some()
            || rpc.binding.identity_id.is_some()
            || !rpc.binding.capability_grants.is_empty()
            || parts
                .headers
                .contains_key(axum::http::header::AUTHORIZATION)
        {
            let principal = state.auth.principal(&parts.headers)?;
            if rpc
                .binding
                .identity_id
                .as_deref()
                .is_some_and(|id| id != principal.identity_id)
            {
                return Err(ApiError::forbidden());
            }
            rpc.binding.identity_id = Some(principal.identity_id.clone());
            check_identity_fields(&rpc.payload, &principal)?;
            if matches!(
                access,
                Access::Author | Access::ObjectOwner | Access::Grant | Access::Revoke
            ) {
                require_author(&rpc.payload, &principal)?;
            }
            if access == Access::Revoke {
                require_grants(
                    state,
                    string(&rpc.payload, "object_id")?,
                    &principal,
                    &[string(&rpc.payload, "grant_id")?.to_owned()],
                )?;
            }
            if access == Access::Sync {
                check_sync(&rpc.payload, &principal)?;
            }
            if access == Access::ObjectOwner {
                require_object_owner(state, string(&rpc.payload, "object_id")?, &principal)?;
            }
            let mut surface_requirement = None;
            if access == Access::Surface || rpc.binding.surface_session_id.is_some() {
                let payload_session = if access == Access::Surface {
                    rpc.payload.get("session_id").and_then(Value::as_str)
                } else {
                    None
                };
                let bound_session = rpc.binding.surface_session_id.as_deref();
                if let (Some(payload), Some(bound)) = (payload_session, bound_session)
                    && payload != bound
                {
                    return Err(ApiError::forbidden());
                }
                let session = bound_session
                    .or(payload_session)
                    .ok_or_else(ApiError::forbidden)?;
                let object = rpc.binding.object_id.clone();
                if access != Access::Surface && object.is_none() {
                    return Err(ApiError::forbidden());
                }
                let requirement = SurfaceAuthorization {
                    session: session.to_owned(),
                    object,
                    document,
                    access: if rpc.binding.object_id.is_some() {
                        SurfaceAccess::Execute
                    } else if rpc.method.as_str() == "babel.runtime.surface.session.heartbeat.v1" {
                        SurfaceAccess::Renew
                    } else if rpc.method.as_str() == "babel.runtime.surface.session.transition.v1"
                        && rpc.payload.get("lifecycle").and_then(Value::as_str) == Some("evicted")
                    {
                        SurfaceAccess::Evict
                    } else if matches!(
                        rpc.method.as_str(),
                        "babel.runtime.surface.session.get.v1"
                            | "babel.runtime.surface.session.state.get.v1"
                    ) {
                        SurfaceAccess::Inspect
                    } else {
                        SurfaceAccess::Manage
                    },
                };
                require_surface_owner(state, &principal, &requirement)?;
                surface_requirement = Some(requirement);
            }
            if access == Access::StartSurface {
                check_start_id(state, &rpc.payload, &principal)?;
            }
            if let Some(object) = rpc.binding.object_id.as_deref() {
                require_grants(state, object, &principal, &rpc.binding.capability_grants)?;
                if access != Access::Public
                    && surface_requirement.is_none()
                    && !owns_object(state, object, &principal)?
                {
                    return Err(ApiError::forbidden());
                }
            } else if rpc.method.as_str().starts_with("babel.storage.")
                || !rpc.binding.capability_grants.is_empty()
            {
                return Err(ApiError::forbidden());
            }
            parts.extensions.insert(ExecutionAuthorization {
                principal: principal.clone(),
                surface: surface_requirement,
            });
            parts.extensions.insert(principal);
        }
        let body =
            serde_json::to_vec(&rpc).map_err(|_| ApiError::internal("RPC encoding failed"))?;
        parts.headers.remove(axum::http::header::CONTENT_LENGTH);
        return Ok(Request::from_parts(parts, Body::from(body)));
    }
    let principal = state.auth.principal(&parts.headers)?;
    let payload: Value = if bytes.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&bytes).map_err(|_| ApiError::bad_request("invalid JSON request"))?
    };
    check_identity_fields(&payload, &principal)?;
    if matches!(
        access,
        Access::Author | Access::ObjectOwner | Access::Grant | Access::Revoke
    ) {
        require_author(&payload, &principal)?;
    }
    if access == Access::Revoke {
        require_grants(
            state,
            string(&payload, "object_id")?,
            &principal,
            &[string(&payload, "grant_id")?.to_owned()],
        )?;
    }
    if access == Access::ObjectOwner {
        require_object_owner(state, string(&payload, "object_id")?, &principal)?;
    }
    if route == "/identities/{id}/keys/rotate"
        && parts.uri.path().split('/').nth(2) != Some(principal.identity_id.as_str())
    {
        return Err(ApiError::forbidden());
    }
    let mut surface_requirement = None;
    if access == Access::Surface {
        let session = parts
            .uri
            .path()
            .split('/')
            .nth(4)
            .ok_or_else(ApiError::forbidden)?;
        let requirement = SurfaceAuthorization {
            session: session.to_owned(),
            object: None,
            document: None,
            access: if route == "/runtime/surfaces/sessions/{id}/document" {
                SurfaceAccess::Execute
            } else if route == "/runtime/surfaces/sessions/{id}/heartbeat" {
                SurfaceAccess::Renew
            } else if route == "/runtime/surfaces/sessions/{id}/lifecycle"
                && parts.method == "POST"
                && payload.get("lifecycle").and_then(Value::as_str) == Some("evicted")
            {
                SurfaceAccess::Evict
            } else if parts.method == "GET" {
                SurfaceAccess::Inspect
            } else {
                SurfaceAccess::Manage
            },
        };
        if payload
            .get("session_id")
            .and_then(Value::as_str)
            .is_some_and(|id| id != session)
        {
            return Err(ApiError::forbidden());
        }
        require_surface_owner(state, &principal, &requirement)?;
        surface_requirement = Some(requirement);
    }
    if access == Access::StartSurface {
        check_start_id(state, &payload, &principal)?;
    }
    if access == Access::Sync {
        if parts.method == "POST" {
            check_sync(&payload, &principal)?;
        } else {
            let Query(query) = Query::<BTreeMap<String, String>>::try_from_uri(&parts.uri)
                .map_err(|_| ApiError::bad_request("invalid query"))?;
            if query.get("identity_id") != Some(&principal.identity_id) {
                return Err(ApiError::forbidden());
            }
        }
    }
    parts.extensions.insert(ExecutionAuthorization {
        principal: principal.clone(),
        surface: surface_requirement,
    });
    parts.extensions.insert(principal);
    let body = if bytes.is_empty() {
        Vec::new()
    } else {
        serde_json::to_vec(&payload).map_err(|_| ApiError::internal("request encoding failed"))?
    };
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    Ok(Request::from_parts(parts, Body::from(body)))
}

fn check_identity_fields(payload: &Value, principal: &Principal) -> Result<(), ApiError> {
    for name in ["author_id", "identity_id"] {
        if let Some(value) = payload.get(name)
            && value.as_str() != Some(principal.identity_id.as_str())
        {
            return Err(ApiError::forbidden());
        }
    }
    Ok(())
}

fn require_author(payload: &Value, principal: &Principal) -> Result<(), ApiError> {
    if string(payload, "author_id")? != principal.identity_id {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

fn check_sync(payload: &Value, principal: &Principal) -> Result<(), ApiError> {
    let identity = payload
        .pointer("/envelope/recipient/identity_id")
        .or_else(|| payload.get("identity_id"))
        .and_then(Value::as_str);
    if identity != Some(principal.identity_id.as_str()) {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

fn string<'a>(payload: &'a Value, name: &str) -> Result<&'a str, ApiError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request(format!("missing {name}")))
}

fn owns_object<P: JudgmentProvider>(
    state: &ApiState<P>,
    object: &str,
    principal: &Principal,
) -> Result<bool, ApiError> {
    let node = lock_node(state)?;
    Ok(node
        .object(&object_id(object.to_owned())?)
        .is_some_and(|object| object.author.as_str() == principal.identity_id))
}

fn require_object_owner<P: JudgmentProvider>(
    state: &ApiState<P>,
    object: &str,
    principal: &Principal,
) -> Result<(), ApiError> {
    if !owns_object(state, object, principal)? {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

fn require_grants<P: JudgmentProvider>(
    state: &ApiState<P>,
    object: &str,
    principal: &Principal,
    grants: &[String],
) -> Result<(), ApiError> {
    if !lock_node(state)?.grants_belong_to(
        &object_id(object.to_owned())?,
        &babel_types::IdentityId::new_unchecked(principal.identity_id.clone()),
        grants,
    )? {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

fn require_surface_owner<P: JudgmentProvider>(
    state: &ApiState<P>,
    principal: &Principal,
    requirement: &SurfaceAuthorization,
) -> Result<(), ApiError> {
    let node = lock_node(state)?;
    surface::require_owner(&state.auth, &node, principal, requirement)
}

fn check_start_id<P: JudgmentProvider>(
    state: &ApiState<P>,
    payload: &Value,
    principal: &Principal,
) -> Result<(), ApiError> {
    let Some(id) = payload.get("session_id").and_then(Value::as_str) else {
        return Ok(());
    };
    let session_id = babel_runtime::SurfaceSessionId::new(id)?;
    let native_exists = lock_node(state)?.surface_session(&session_id).is_ok();
    let stored = state.auth.with_store(|store| store.has_surface(id))?;
    if native_exists || stored {
        let owned = state.auth.with_store(|store| {
            store.owns_surface(id, principal, Some(string(payload, "object_id")?), false)
        })?;
        if !owned {
            return Err(ApiError::forbidden());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_catalog_policy_is_explicit_and_surface_mutations_require_capabilities() {
        let catalog = babel_rpc::babel_rpc_catalog().unwrap();
        for method in catalog.methods {
            let name = method.method.as_str();
            if name != "babel.identity.create.v1" {
                assert!(
                    rpc_access(name) != Access::Denied,
                    "classify {name} explicitly"
                );
            }
            if surface_method(name) && rpc_access(name) != Access::Surface {
                assert!(
                    method
                        .capability
                        .is_some_and(|capability| capability.required),
                    "Surface mutation must require capability: {name}"
                );
            }
        }
        assert!(rpc_access("babel.future.write.v1") == Access::Denied);
        assert!(rest_access("POST", "/future/admin") == Access::Denied);
    }
}
