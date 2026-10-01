//! Host-only consent transport. The node stores intent and consumes it with effects.
mod context;
pub(crate) mod browser;
pub(crate) mod schema;
#[cfg(test)]
mod tests;

use crate::{
    ApiState,
    auth::Principal,
    error::ApiError,
    routes::{lock_node, object_id},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use babel_capabilities::invocation::{
    InvocationContext, InvocationId, InvocationInvalidation, InvocationOrigin, InvocationRecord,
    InvocationState,
};
use babel_judgment::JudgmentProvider;
use babel_node::{LocalNode, SocialInvocationPayload, SocialInvocationResult};
use babel_rpc::{RpcError, RpcErrorCode, RpcRequestEnvelope};
use babel_types::Timestamp;
use schema::*;
use serde_json::Value;

pub(crate) fn router<P: JudgmentProvider + Send + Sync + 'static>() -> Router<ApiState<P>> {
    Router::new()
        .merge(browser::router::<P>())
        .route(
            "/invocations/v1/documents/{document_id}",
            put(register::<P>).delete(retire::<P>),
        )
        .route("/invocations/v1/prepare", post(prepare::<P>))
        .route("/invocations/v1/recover", post(recover::<P>))
        .route("/invocations/v1/{id}/decision", post(decide::<P>))
        .route("/invocations/v1/{id}/execute", post(execute::<P>))
        .route("/invocations/v1/{id}/cancel", post(cancel::<P>))
        .route("/invocations/v1/{id}/status", get(status::<P>))
        .layer(axum::middleware::from_fn(no_store))
}

async fn no_store(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

async fn register<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(document): Path<String>,
    headers: HeaderMap,
    Json(input): Json<RegisterHostDocumentRequest>,
) -> Result<Json<HostDocumentResponse>, ApiError> {
    crate::auth::surface::validate_document(&document)?;
    let node = lock_node(&state)?;
    let principal = state.auth.principal(&headers)?;
    let object = object_id(input.object_id)?;
    let controller = node
        .object(&object)
        .ok_or_else(|| ApiError::not_found("controller Object not found"))?;
    if controller.author.as_str() != principal.identity_id {
        return Err(ApiError::forbidden());
    }
    let expires = state.auth.with_store(|store| {
        store.register_host_document(
            &principal,
            &document,
            object.as_str(),
            node.invocation_epoch().as_str(),
        )
    })?;
    Ok(Json(HostDocumentResponse {
        document_id: document,
        object_id: object.to_string(),
        expires_at: Timestamp(
            time::OffsetDateTime::from_unix_timestamp(expires)
                .map_err(|_| ApiError::internal("invalid host document expiry"))?,
        ),
        renew_after_ms: 20_000,
    }))
}

async fn retire<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(document): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let mut node = lock_node(&state)?;
    let principal = state.auth.principal(&headers)?;
    state
        .auth
        .with_store(|store| store.retire_host_document(&principal, &document))?;
    invalidate_document(&mut node, &principal, &document)?;
    Ok(StatusCode::NO_CONTENT)
}

fn invalidate_document<P: JudgmentProvider>(
    node: &mut LocalNode<P>,
    principal: &Principal,
    document: &str,
) -> Result<(), ApiError> {
    for record in node.store().list_invocations()? {
        let ctx = &record.intent().context;
        if ctx.login_id == principal.account_session
            && matches!(&ctx.origin, InvocationOrigin::HostAction { document_id } if document_id == document)
            && matches!(
                record.state(),
                InvocationState::Pending | InvocationState::Approved | InvocationState::Running { .. }
            )
        {
            node.invalidate_social_invocations(ctx, InvocationInvalidation::ContextLost)?;
        }
    }
    Ok(())
}

pub(crate) fn cleanup_host_contexts<P: JudgmentProvider>(
    auth: &crate::auth::Auth,
    node: &mut LocalNode<P>,
) -> Result<(), ApiError> {
    for record in node.store().list_invocations()? {
        if !matches!(
            record.state(),
            InvocationState::Pending | InvocationState::Approved | InvocationState::Running { .. }
        ) {
            continue;
        }
        let ctx = &record.intent().context;
        let InvocationOrigin::HostAction { document_id } = &ctx.origin else {
            continue;
        };
        let valid = auth.with_store(|store| {
            let principal = store.authenticate_hash(&ctx.login_id)?;
            store.require_host_document(
                &principal,
                document_id,
                ctx.object_id.as_str(),
                ctx.context_epoch.as_str(),
            )
        });
        match valid {
            Ok(()) => (),
            Err(error) if matches!(error.code(), "unauthorized" | "forbidden") => {
                node.invalidate_social_invocations(ctx, InvocationInvalidation::ContextLost)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

async fn prepare<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
    Json(input): Json<PrepareInvocationRequest>,
) -> Result<Json<InvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let principal = state.auth.principal(&headers)?;
    context::require_header(&headers, &input.origin)?;
    let ctx = context::current(&state, &node, &principal, &input.object_id, &input.origin)?;
    prepare_locked(&mut node, ctx, input).map(Json)
}

async fn recover<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
    Json(input): Json<RecoverInvocationRequest>,
) -> Result<Response, ApiError> {
    let node = lock_node(&state)?;
    let principal = state.auth.principal(&headers)?;
    if headers.contains_key("x-babel-host-document")
        || headers.contains_key(crate::auth::surface::DOCUMENT_HEADER)
    {
        return Err(ApiError::forbidden());
    }
    if !social_method(&input.method) {
        return Err(ApiError::bad_request("unsupported invocation method"));
    }
    let object = object_id(input.object_id)?;
    let controller = node
        .object(&object)
        .ok_or_else(|| ApiError::not_found("controller Object not found"))?;
    if controller.author.as_str() != principal.identity_id {
        return Err(ApiError::forbidden());
    }
    let payload = normalize_payload(
        &principal.identity_id,
        &object,
        &input.method,
        input.payload,
    )?;
    let actor = babel_types::IdentityId::new_unchecked(principal.identity_id);
    // Hydration only: no document rebinding, policy refresh, or state transition.
    match node.recover_social_invocation(
        &actor,
        &principal.account_session,
        &object,
        &input.request_key,
        &input.method,
        payload,
    )? {
        Some(result) => Ok(Json(executed_view(result)).into_response()),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

fn prepare_locked<P: JudgmentProvider>(
    node: &mut LocalNode<P>,
    ctx: InvocationContext,
    input: PrepareInvocationRequest,
) -> Result<InvocationResponse, ApiError> {
    if !social_method(&input.method) {
        return Err(ApiError::bad_request("unsupported invocation method"));
    }
    if input.timeout_ms == 0 || input.timeout_ms > 30_000 {
        return Err(ApiError::bad_request("timeout_ms must be 1..30000"));
    }
    let payload = normalize_payload(
        ctx.actor.as_str(),
        &ctx.object_id,
        &input.method,
        input.payload,
    )?;
    let ingress = crate::execution::INGRESS
        .try_with(|value| *value)
        .unwrap_or_else(|_| Timestamp::now());
    let deadline = Timestamp(ingress.0 + time::Duration::milliseconds(input.timeout_ms as i64));
    let record = node.prepare_social_invocation(
        ctx.clone(),
        &input.request_key,
        &input.method,
        payload,
        deadline,
    )?;
    view_with_result(node, record, &ctx)
}

fn normalize_payload(
    actor: &str,
    object: &babel_types::ObjectId,
    method: &str,
    value: Value,
) -> Result<SocialInvocationPayload, ApiError> {
    let author = value
        .get("author_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request("author_id required"))?;
    if author != actor {
        return Err(ApiError::forbidden());
    }
    let text_method = matches!(method, "babel.social.share.v2" | "babel.social.reply.v2");
    let fields = value
        .as_object()
        .ok_or_else(|| ApiError::bad_request("payload must be an object"))?;
    if fields.keys().any(|key| {
        !matches!(key.as_str(), "author_id" | "target_object_id")
            && !(text_method && matches!(key.as_str(), "text" | "media"))
    }) {
        return Err(ApiError::bad_request("unknown social payload field"));
    }
    if text_method {
        let input: crate::SocialTextRequest =
            serde_json::from_value(value).map_err(|e| ApiError::bad_request(e.to_string()))?;
        Ok(SocialInvocationPayload {
            target_object_id: input
                .target_object_id
                .map(object_id)
                .transpose()?
                .unwrap_or_else(|| object.clone()),
            text: Some(input.text),
            media: input.media,
        })
    } else {
        let input: crate::SocialTargetRequest =
            serde_json::from_value(value).map_err(|e| ApiError::bad_request(e.to_string()))?;
        Ok(SocialInvocationPayload {
            target_object_id: object_id(input.target_object_id)?,
            text: None,
            media: None,
        })
    }
}

async fn decide<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
    Json(input): Json<DecideInvocationRequest>,
) -> Result<Json<InvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked_record(&state, &mut node, &headers, &id)?;
    let next = node.decide_social_invocation(
        &ctx,
        &record.intent().request_key,
        &id,
        matches!(input.decision, InvocationDecision::AllowOnce),
    )?;
    view_with_result(&mut node, next, &ctx).map(Json)
}

async fn execute<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
) -> Result<Json<InvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked_record(&state, &mut node, &headers, &id)?;
    let result = node.execute_social_invocation(&ctx, &record.intent().request_key, &id)?;
    Ok(Json(executed_view(result)))
}

async fn cancel<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
) -> Result<Json<InvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked_record(&state, &mut node, &headers, &id)?;
    let next = node.cancel_social_invocation(&ctx, &record.intent().request_key, &id)?;
    view_with_result(&mut node, next, &ctx).map(Json)
}

async fn status<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
) -> Result<Json<InvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked_record(&state, &mut node, &headers, &id)?;
    let current = node
        .status_social_invocation(&ctx, &record.intent().request_key)?
        .ok_or_else(|| ApiError::not_found("invocation not found"))?;
    view_with_result(&mut node, current, &ctx).map(Json)
}

fn checked_record<P: JudgmentProvider>(
    state: &ApiState<P>,
    node: &mut LocalNode<P>,
    headers: &HeaderMap,
    id: &InvocationId,
) -> Result<(InvocationRecord, InvocationContext), ApiError> {
    let principal = state.auth.principal(headers)?;
    let record = node
        .invocation_by_id(id)?
        .ok_or_else(|| ApiError::not_found("invocation not found"))?;
    let stored = &record.intent().context;
    if stored.actor.as_str() != principal.identity_id
        || stored.login_id != principal.account_session
    {
        return Err(ApiError::forbidden());
    }
    let source = source(&stored.origin);
    context::require_header(headers, &source)?;
    let ctx = match context::current(state, node, &principal, stored.object_id.as_str(), &source) {
        Ok(current) if current == *stored => current,
        Ok(_) => {
            node.invalidate_social_invocations(stored, InvocationInvalidation::ContextLost)?;
            return Err(ApiError::forbidden());
        }
        Err(error) if matches!(error.code(), "unauthorized" | "forbidden" | "not_found") => {
            node.invalidate_social_invocations(stored, InvocationInvalidation::ContextLost)?;
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    Ok((record, ctx))
}

fn view_with_result<P: JudgmentProvider>(
    node: &mut LocalNode<P>,
    record: InvocationRecord,
    _ctx: &InvocationContext,
) -> Result<InvocationResponse, ApiError> {
    if matches!(record.state(), InvocationState::Completed { .. }) {
        Ok(executed_view(node.social_invocation_result(record)?))
    } else {
        Ok(view(record))
    }
}

fn executed_view(result: SocialInvocationResult) -> InvocationResponse {
    let mut response = view(result.invocation);
    response.result = Some(InvocationSocialResult {
        object: result.object,
        edge: result.edge,
        receipt: result.receipt,
    });
    response
}

fn source(origin: &InvocationOrigin) -> InvocationSource {
    match origin {
        InvocationOrigin::HostAction { document_id } => InvocationSource::HostAction {
            document_id: document_id.clone(),
        },
        InvocationOrigin::Surface {
            session_id,
            document_id,
            ..
        } => InvocationSource::Surface {
            session_id: session_id.clone(),
            document_id: document_id.clone(),
        },
    }
}

fn view(record: InvocationRecord) -> InvocationResponse {
    let intent = record.intent();
    InvocationResponse {
        invocation_id: record.id().clone(),
        request_key: intent.request_key.clone(),
        actor_id: intent.context.actor.to_string(),
        object_id: intent.context.object_id.to_string(),
        origin: source(&intent.context.origin),
        method: intent.method.clone(),
        payload: intent.payload.clone(),
        created_at: intent.created_at,
        deadline: intent.deadline,
        state: record.state().clone(),
        revision: record.revision(),
        result: None,
    }
}

pub(crate) fn social_method(method: &str) -> bool {
    matches!(
        method,
        "babel.social.follow.v2"
            | "babel.social.unfollow.v2"
            | "babel.social.share.v2"
            | "babel.social.reply.v2"
    )
}

pub(crate) fn intercept<P: JudgmentProvider>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
    principal: Option<&Principal>,
) -> Result<Value, RpcError> {
    let work = || -> Result<InvocationResponse, ApiError> {
        let principal = principal.ok_or_else(ApiError::forbidden)?;
        let binding = crate::auth::surface::EXECUTION
            .try_with(|ctx| ctx.surface.clone())
            .ok()
            .flatten()
            .ok_or_else(ApiError::forbidden)?;
        let origin = InvocationSource::Surface {
            session_id: binding.session,
            document_id: binding.document.ok_or_else(ApiError::forbidden)?,
        };
        let object = request
            .binding
            .object_id
            .clone()
            .ok_or_else(ApiError::forbidden)?;
        let mut node = lock_node(state)?;
        let ctx = context::current(state, &node, principal, &object, &origin)?;
        prepare_locked(
            &mut node,
            ctx,
            PrepareInvocationRequest {
                origin,
                object_id: object,
                method: request.method.as_str().into(),
                request_key: request
                    .idempotency_key
                    .clone()
                    .ok_or_else(|| ApiError::bad_request("idempotency key required"))?,
                payload: request.payload.clone(),
                timeout_ms: request.deadline.timeout_ms,
            },
        )
    };
    let view = work().map_err(rpc_error)?;
    if let Some(result) = view.result {
        return serde_json::to_value(result)
            .map_err(|e| RpcError::new(RpcErrorCode::Internal, e.to_string()));
    }
    if !matches!(
        view.state,
        InvocationState::Pending | InvocationState::Approved
    ) {
        return Err(
            RpcError::new(RpcErrorCode::CapabilityDenied, "invocation is terminal")
                .with_details(serde_json::json!({"invocation":view})),
        );
    }
    Err(RpcError::new(
        RpcErrorCode::PermissionRequired,
        "host approval required for this invocation",
    )
    .with_details(serde_json::json!({"invocation":view})))
}

fn rpc_error(error: ApiError) -> RpcError {
    let code = match error.code() {
        "forbidden" | "unauthorized" => RpcErrorCode::CapabilityDenied,
        "bad_request" => RpcErrorCode::InvalidInput,
        "not_found" => RpcErrorCode::NotFound,
        "conflict" => RpcErrorCode::Conflict,
        "provider_unavailable" => RpcErrorCode::CapabilityUnavailable,
        "storage_unavailable" => RpcErrorCode::StorageUnavailable,
        _ => RpcErrorCode::Internal,
    };
    RpcError::new(code, error.message())
}
