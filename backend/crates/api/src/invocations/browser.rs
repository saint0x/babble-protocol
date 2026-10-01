//! Host-only browser invocation transport over the canonical journal/context.
use super::*;
use babble_capabilities::invocation::{InvocationAction, InvocationOutcome, browser::*};

pub(super) fn router<P: JudgmentProvider + Send + Sync + 'static>() -> Router<ApiState<P>> {
    Router::new()
        .route("/invocations/v1/browser/prepare", post(prepare::<P>))
        .route("/invocations/v1/browser/{id}/decision", post(decide::<P>))
        .route("/invocations/v1/browser/{id}/dispatch", post(dispatch::<P>))
        .route("/invocations/v1/browser/{id}/ack", post(ack::<P>))
        .route("/invocations/v1/browser/{id}/status", get(status::<P>))
        .route("/invocations/v1/browser/{id}/cancel", post(cancel::<P>))
}

pub(crate) fn browser_method(method: &str) -> bool {
    matches!(
        method,
        "babble.clipboard.write" | "babble.fullscreen.enter"
    )
}

async fn prepare<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
    Json(input): Json<PrepareInvocationRequest>,
) -> Result<Json<BrowserInvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let principal = state.auth.principal(&headers)?;
    context::require_header(&headers, &input.origin)?;
    let ctx = context::current(&state, &node, &principal, &input.object_id, &input.origin)?;
    prepare_locked(&mut node, ctx, input).map(Json)
}

fn prepare_locked<P: JudgmentProvider>(
    node: &mut LocalNode<P>,
    ctx: InvocationContext,
    input: PrepareInvocationRequest,
) -> Result<BrowserInvocationResponse, ApiError> {
    if !browser_method(&input.method) || input.timeout_ms == 0 || input.timeout_ms > 30_000 {
        return Err(ApiError::bad_request(
            "unsupported browser method or timeout_ms outside 1..30000",
        ));
    }
    let ingress = crate::execution::INGRESS
        .try_with(|t| *t)
        .unwrap_or_else(|_| Timestamp::now());
    let deadline = Timestamp(ingress.0 + time::Duration::milliseconds(input.timeout_ms as i64));
    let record = node.prepare_browser_invocation(
        ctx,
        &input.request_key,
        &input.method,
        input.payload,
        deadline,
    )?;
    view(record, None)
}

fn checked<P: JudgmentProvider>(
    state: &ApiState<P>,
    node: &mut LocalNode<P>,
    headers: &HeaderMap,
    id: &InvocationId,
    live: bool,
) -> Result<(InvocationRecord, InvocationContext), ApiError> {
    let principal = state.auth.principal(headers)?;
    let record = node
        .invocation_by_id(id)?
        .ok_or_else(|| ApiError::not_found("invocation not found"))?;
    let ctx = &record.intent().context;
    if ctx.actor.as_str() != principal.identity_id || ctx.login_id != principal.account_session {
        return Err(ApiError::forbidden());
    }
    context::require_header(headers, &source(&ctx.origin))?;
    if !browser_method(&record.intent().method) {
        return Err(ApiError::bad_request("not a browser invocation"));
    }
    if live {
        return super::checked_record(state, node, headers, id);
    }
    if matches!(
        record.state(),
        InvocationState::Pending | InvocationState::Approved | InvocationState::Running { .. }
    ) {
        let current = context::current(
            state,
            node,
            &principal,
            ctx.object_id.as_str(),
            &source(&ctx.origin),
        );
        match current {
            Ok(current) if &current == ctx => (),
            Ok(_) => {
                node.invalidate_social_invocations(ctx, InvocationInvalidation::ContextLost)?;
            }
            Err(error) if matches!(error.code(), "unauthorized" | "forbidden" | "not_found") => {
                node.invalidate_social_invocations(ctx, InvocationInvalidation::ContextLost)?;
            }
            Err(error) => return Err(error),
        }
        let context = ctx.clone();
        let record = node
            .invocation_by_id(id)?
            .ok_or_else(|| ApiError::not_found("invocation not found"))?;
        return Ok((record, context));
    }
    // Outcome reporting/history does not authorize execution. The original
    // actor, originating login and exact document remain mandatory after loss.
    Ok((record.clone(), ctx.clone()))
}

async fn decide<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
    Json(input): Json<DecideInvocationRequest>,
) -> Result<Json<BrowserInvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked(&state, &mut node, &headers, &id, true)?;
    let record = node.decide_browser_invocation(
        &ctx,
        &record.intent().request_key,
        &id,
        matches!(input.decision, InvocationDecision::AllowOnce),
    )?;
    view(record, None).map(Json)
}

async fn dispatch<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
) -> Result<Json<BrowserInvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked(&state, &mut node, &headers, &id, true)?;
    let (record, ticket) =
        node.dispatch_browser_invocation(&ctx, &record.intent().request_key, &id)?;
    view(record, ticket).map(Json)
}

async fn ack<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
    Json(input): Json<AcknowledgeBrowserInvocationRequest>,
) -> Result<Json<BrowserInvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked(&state, &mut node, &headers, &id, false)?;
    let record =
        node.acknowledge_browser_invocation(&ctx, &record.intent().request_key, &id, input)?;
    view(record, None).map(Json)
}

async fn status<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
) -> Result<Json<BrowserInvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked(&state, &mut node, &headers, &id, false)?;
    let record = node.status_browser_invocation(&ctx, &record.intent().request_key, &id)?;
    view(record, None).map(Json)
}

async fn cancel<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path(id): Path<InvocationId>,
    headers: HeaderMap,
) -> Result<Json<BrowserInvocationResponse>, ApiError> {
    let mut node = lock_node(&state)?;
    let (record, ctx) = checked(&state, &mut node, &headers, &id, false)?;
    let record = node.cancel_browser_invocation(&ctx, &record.intent().request_key, &id)?;
    view(record, None).map(Json)
}

fn view(
    record: InvocationRecord,
    execution_ticket: Option<BrowserExecutionTicket>,
) -> Result<BrowserInvocationResponse, ApiError> {
    let result = match record.state() {
        InvocationState::Completed {
            outcome: InvocationOutcome::External { result, .. },
        } => Some(
            serde_json::from_value(result.clone())
                .map_err(|e| ApiError::internal(e.to_string()))?,
        ),
        InvocationState::Failed { code }
            if matches!(record.action(), Some(InvocationAction::FailExternal { .. })) =>
        {
            Some(
                serde_json::from_value(serde_json::json!({"kind":"failed","code":code}))
                    .map_err(|e| ApiError::internal(e.to_string()))?,
            )
        }
        _ => None,
    };
    let intent = record.intent();
    Ok(BrowserInvocationResponse {
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
        result,
        execution_ticket,
    })
}

pub(crate) fn intercept<P: JudgmentProvider>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
    principal: Option<&Principal>,
) -> Result<Value, RpcError> {
    let work = || -> Result<BrowserInvocationResponse, ApiError> {
        let principal = principal.ok_or_else(ApiError::forbidden)?;
        let binding = crate::auth::surface::EXECUTION
            .try_with(|c| c.surface.clone())
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
    let response = work().map_err(super::rpc_error)?;
    if matches!(response.state, InvocationState::Completed { .. }) {
        return serde_json::to_value(response.result)
            .map_err(|e| RpcError::new(RpcErrorCode::Internal, e.to_string()));
    }
    let pending = matches!(
        response.state,
        InvocationState::Pending | InvocationState::Approved
    );
    Err(RpcError::new(
        if pending {
            RpcErrorCode::PermissionRequired
        } else {
            RpcErrorCode::CapabilityDenied
        },
        if pending {
            "host approval required for browser invocation"
        } else {
            "browser invocation cannot redispatch"
        },
    )
    .with_details(serde_json::json!({"invocation":response})))
}
