use crate::{
    error::ApiError,
    routes::{ApiState, lock_node},
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use babble_judgment::JudgmentProvider;
use babble_node::{QuotesListQuery, QuotesListResult};
use babble_types::ObjectId;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuoteParameters {
    cursor: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn list<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Query(parameters): Query<QuoteParameters>,
) -> Result<Json<QuotesListResult>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let node = lock_node(&state)?;
    Ok(Json(node.list_quotes(&QuotesListQuery {
        object_id: ObjectId::new_unchecked(id),
        cursor: parameters.cursor,
        limit: parameters.limit.unwrap_or(20),
    })?))
}

#[cfg(test)]
#[path = "tests/quotes.rs"]
mod tests;
