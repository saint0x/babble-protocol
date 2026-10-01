use crate::error::ApiError;
use axum::http::HeaderMap;
use babel_store::PublicationRequest;
use babel_types::{Canonical, IdentityId};
use serde::Serialize;

/// REST callers opt into retry safety; RPC already requires its envelope key.
pub(crate) fn rest_request(
    headers: &HeaderMap,
    author: &IdentityId,
    method: &str,
    payload: &impl Serialize,
) -> Result<Option<PublicationRequest>, ApiError> {
    let mut keys = headers.get_all("idempotency-key").iter();
    let Some(value) = keys.next() else {
        return Ok(None);
    };
    if keys.next().is_some() {
        return Err(ApiError::bad_request(
            "exactly one Idempotency-Key is allowed",
        ));
    }
    let key = value
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid Idempotency-Key"))?;
    if key.trim().is_empty() || key.len() > 256 {
        return Err(ApiError::bad_request("Idempotency-Key must be 1-256 bytes"));
    }
    Ok(Some(PublicationRequest {
        id:
            serde_json::json!({"version":1,"domain":"babel.consent.rest","author":author,"key":key})
                .canonical_hash()?,
        fingerprint: serde_json::json!({"version":1,"method":method,"payload":payload})
            .canonical_hash()?,
        author: author.clone(),
    }))
}
