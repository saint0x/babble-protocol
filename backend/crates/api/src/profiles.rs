use crate::{
    error::ApiError,
    routes::{ApiState, lock_node},
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use babble_judgment::JudgmentProvider;
use babble_node::{AuthorObjectsPage, AuthorObjectsQuery};
use babble_types::IdentityId;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileParameters {
    cursor: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn author_objects<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Query(parameters): Query<ProfileParameters>,
) -> Result<Json<AuthorObjectsPage>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let node = lock_node(&state)?;
    Ok(Json(node.author_objects(&AuthorObjectsQuery {
        identity_id: IdentityId::new_unchecked(id),
        cursor: parameters.cursor,
        limit: parameters.limit.unwrap_or(20),
    })?))
}
