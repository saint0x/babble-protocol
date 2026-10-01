use crate::{
    auth::Principal,
    error::ApiError,
    routes::{ApiState, lock_node},
};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use babble_judgment::JudgmentProvider;
use babble_node::{SafetySnapshot, SafetyState};
use babble_types::IdentityId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSafetyRequest {
    pub blocked: bool,
    pub muted: bool,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub expected_revision: u64,
    #[schemars(length(min = 1, max = 256))]
    pub idempotency_key: String,
}

pub(crate) async fn snapshot<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<SafetySnapshot>, ApiError> {
    Ok(Json(lock_node(&state)?.safety_snapshot(
        &IdentityId::new_unchecked(principal.identity_id),
    )?))
}

pub(crate) async fn state<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(target): Path<String>,
) -> Result<Json<SafetyState>, ApiError> {
    Ok(Json(lock_node(&state)?.safety_state(
        &IdentityId::new_unchecked(principal.identity_id),
        &IdentityId::new_unchecked(target),
    )?))
}

pub(crate) async fn set<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Extension(principal): Extension<Principal>,
    Path(target): Path<String>,
    Json(input): Json<SetSafetyRequest>,
) -> Result<Json<SafetyState>, ApiError> {
    Ok(Json(lock_node(&state)?.set_safety(
        &IdentityId::new_unchecked(principal.identity_id),
        &IdentityId::new_unchecked(target),
        input.blocked,
        input.muted,
        input.expected_revision,
        &input.idempotency_key,
    )?))
}
