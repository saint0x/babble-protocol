use crate::{
    auth::Principal,
    error::ApiError,
    routes::{ApiState, identity_id, lock_node, object_id},
};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use babel_graph::{ReactionRecord, ReactionState, ReactionSummary, ReactionValue};
use babel_judgment::JudgmentProvider;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetReactionRequest {
    pub value: ReactionValue,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub expected_revision: u64,
    #[schemars(length(min = 1, max = 256))]
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReactionObjectRequest {
    pub object_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReactionRecordRequest {
    pub object_id: String,
    pub actor_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetReactionRpcRequest {
    pub object_id: String,
    pub value: ReactionValue,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub expected_revision: u64,
}

pub(crate) async fn summary<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<ReactionSummary>, ApiError> {
    Ok(Json(lock_node(&state)?.reaction_summary(&object_id(id)?)?))
}

pub(crate) async fn record<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Path((id, actor)): Path<(String, String)>,
) -> Result<Json<ReactionRecord>, ApiError> {
    Ok(Json(
        lock_node(&state)?.reaction_record(&identity_id(actor)?, &object_id(id)?)?,
    ))
}

pub(crate) async fn mine<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<ReactionState>, ApiError> {
    Ok(Json(lock_node(&state)?.reaction_state(
        &identity_id(principal.identity_id)?,
        &object_id(id)?,
    )?))
}

pub(crate) async fn set<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(input): Json<SetReactionRequest>,
) -> Result<Json<ReactionState>, ApiError> {
    Ok(Json(lock_node(&state)?.set_reaction(
        &identity_id(principal.identity_id)?,
        &object_id(id)?,
        input.value,
        input.expected_revision,
        &input.idempotency_key,
    )?))
}
