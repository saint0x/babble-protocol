use crate::{
    auth::Principal,
    error::ApiError,
    routes::{ApiState, lock_node},
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use babble_judgment::JudgmentProvider;
use babble_node::{FollowListPage, FollowState, FollowingPage, FollowingQuery};
use babble_types::IdentityId;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFollowingRequest {
    pub following: bool,
    pub expected_revision: u64,
    #[schemars(length(min = 1, max = 256))]
    pub idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListParameters {
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeedParameters {
    cursor: Option<String>,
    limit: Option<usize>,
    search: Option<String>,
}

pub(crate) async fn state<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(target): Path<String>,
) -> Result<Json<FollowState>, ApiError> {
    let node = lock_node(&state)?;
    Ok(Json(node.follow_state(
        &IdentityId::new_unchecked(principal.identity_id),
        &IdentityId::new_unchecked(target),
    )?))
}

pub(crate) async fn set<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(target): Path<String>,
    Json(input): Json<SetFollowingRequest>,
) -> Result<Json<FollowState>, ApiError> {
    let mut node = lock_node(&state)?;
    Ok(Json(node.set_following(
        &IdentityId::new_unchecked(principal.identity_id),
        &IdentityId::new_unchecked(target),
        input.following,
        input.expected_revision,
        &input.idempotency_key,
    )?))
}

pub(crate) async fn list<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Query(parameters): Query<ListParameters>,
) -> Result<Json<FollowListPage>, ApiError> {
    let node = lock_node(&state)?;
    Ok(Json(node.list_following(
        &IdentityId::new_unchecked(principal.identity_id),
        parameters.cursor.as_deref(),
        parameters.limit.unwrap_or(20),
    )?))
}

pub(crate) async fn feed<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Query(parameters): Query<FeedParameters>,
) -> Result<Json<FollowingPage>, ApiError> {
    let node = lock_node(&state)?;
    Ok(Json(node.following_feed(
        &IdentityId::new_unchecked(principal.identity_id),
        &FollowingQuery {
            limit: parameters.limit.unwrap_or(20),
            cursor: parameters.cursor,
            search: parameters.search,
        },
    )?))
}
