use crate::{
    auth::Principal,
    error::ApiError,
    routes::{ApiState, lock_node},
};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use babble_graph::moderation::*;
use babble_judgment::JudgmentProvider;
use babble_types::IdentityId;
use serde::Deserialize;

pub(crate) fn router<P: JudgmentProvider + Send + Sync + 'static>() -> Router<ApiState<P>> {
    Router::new()
        .route("/moderation/access", get(access::<P>))
        .route("/moderation/reports", get(list::<P>).post(report::<P>))
        .route("/moderation/reports/{id}", get(detail::<P>))
        .route("/moderation/reports/{id}/decisions", post(decide::<P>))
        .route("/moderation/reports/{id}/appeals", post(appeal::<P>))
}
fn actor(principal: Principal) -> IdentityId {
    IdentityId::new_unchecked(principal.identity_id)
}
async fn access<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<ModerationAccess>, ApiError> {
    Ok(Json(
        lock_node(&state)?.moderation_access(&actor(principal))?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    #[serde(default = "scope")]
    scope: ModerationScope,
    before: Option<u64>,
    #[serde(default = "limit")]
    limit: usize,
}
fn scope() -> ModerationScope {
    ModerationScope::Mine
}
fn limit() -> usize {
    25
}
async fn list<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ModerationPage>, ApiError> {
    let node = lock_node(&state)?;
    let actor = actor(principal);
    if matches!(query.scope, ModerationScope::Queue) && !node.moderation_access(&actor)?.can_review
    {
        return Err(ApiError::forbidden());
    }
    Ok(Json(node.moderation_list(
        &actor,
        query.scope,
        query.before,
        query.limit,
    )?))
}
async fn detail<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<ModerationCase>, ApiError> {
    Ok(Json(
        lock_node(&state)?.moderation_case(&actor(principal), &id)?,
    ))
}
async fn report<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Json(request): Json<ReportRequest>,
) -> Result<Json<ModerationCase>, ApiError> {
    let result = lock_node(&state)?.moderation_report(&actor(principal),request).map_err(|error| {
        if matches!(&error,babble_types::Error::Conflict(message) if message == REPORT_INTAKE_LIMIT) { ApiError::report_intake_limit() } else { error.into() }
    })?;
    Ok(Json(result))
}
async fn decide<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(request): Json<DecisionRequest>,
) -> Result<Json<ModerationCase>, ApiError> {
    let mut node = lock_node(&state)?;
    let actor = actor(principal);
    if !node.moderation_access(&actor)?.can_review {
        return Err(ApiError::forbidden());
    }
    let case = node.moderation_case(&actor, &id)?;
    if case.reporter_id.as_ref() == Some(&actor)
        || case.subject_author_id == actor
        || (request.expected_revision == 3
            && case
                .decisions
                .first()
                .is_some_and(|d| d.reviewer_id == actor))
    {
        return Err(ApiError::forbidden());
    }
    Ok(Json(node.moderation_decide(&actor, id, request)?))
}
async fn appeal<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(request): Json<AppealRequest>,
) -> Result<Json<ModerationCase>, ApiError> {
    Ok(Json(lock_node(&state)?.moderation_appeal(
        &actor(principal),
        id,
        request,
    )?))
}
