use super::{Auth, password_engine, password_policy, surface};
use crate::{ApiState, error::ApiError, routes::lock_node};
use argon2::{PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use babble_judgment::JudgmentProvider;
use rand_core::OsRng;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["id", "created_at", "expires_at", "current"]))]
pub struct AccountSessionInfo {
    #[schemars(regex(pattern = "^account_[a-f0-9]{64}$"))]
    pub id: String,
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(extend("format" = "date-time"))]
    pub created_at: Option<String>,
    #[schemars(extend("format" = "date-time"))]
    pub expires_at: String,
    pub current: bool,
}

fn required_nullable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::deserialize(deserializer)
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountSessionsResponse {
    #[schemars(length(max = 16))]
    pub sessions: Vec<AccountSessionInfo>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub(super) async fn sessions<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
) -> Result<Json<AccountSessionsResponse>, ApiError> {
    let _node = lock_node(&state)?;
    let principal = state.auth.principal(&headers)?;
    let sessions = state.auth.with_store(|store| store.sessions(&principal))?;
    Ok(Json(AccountSessionsResponse { sessions }))
}

pub(super) async fn revoke_session<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    revoke(&state, &headers, Some(&id))
}

pub(super) async fn revoke_others<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    if !body.is_empty() {
        // Authenticate first: malformed input must not bypass the host boundary.
        state.auth.principal(&headers)?;
        return Err(ApiError::bad_request("request body must be empty"));
    }
    revoke(&state, &headers, None)
}

fn revoke<P: JudgmentProvider>(
    state: &ApiState<P>,
    headers: &HeaderMap,
    id: Option<&str>,
) -> Result<StatusCode, ApiError> {
    let mut node = lock_node(state)?;
    let principal = state.auth.principal(headers)?;
    let origins = state
        .auth
        .with_store(|store| store.revoke_sessions(&principal, id))?;
    cleanup_committed(&state.auth, &mut node, &origins);
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn change_password<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
    Json(input): Json<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    let current = Zeroizing::new(input.current_password);
    let new = Zeroizing::new(input.new_password);
    let (principal, observed) = {
        let _node = lock_node(&state)?;
        let principal = state.auth.principal(&headers)?;
        let observed = state
            .auth
            .with_store(|store| store.password_hash(&principal.identity_id))?
            .ok_or_else(ApiError::unauthorized)?;
        (principal, Zeroizing::new(observed))
    };
    password_policy(&new)?;
    if *new == *current {
        return Err(ApiError::bad_request(
            "new password must differ from the current password",
        ));
    }
    let permit = state.auth.password_slot()?;
    let (observed, replacement) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let valid = current.len() <= 1024
            && PasswordHash::new(&observed).ok().is_some_and(|hash| {
                password_engine()
                    .verify_password(current.as_bytes(), &hash)
                    .is_ok()
            });
        let replacement = if valid {
            let salt = SaltString::generate(&mut OsRng);
            Some(Zeroizing::new(
                password_engine()
                    .hash_password(new.as_bytes(), &salt)
                    .map_err(|_| ApiError::internal("password hashing failed"))?
                    .to_string(),
            ))
        } else {
            None
        };
        Ok::<_, ApiError>((observed, replacement))
    })
    .await
    .map_err(|_| ApiError::internal("password worker failed"))??;
    let mut node = lock_node(&state)?;
    let origins = state.auth.with_store(|store| {
        store.change_password(
            &principal,
            &observed,
            replacement.as_ref().map(|hash| hash.as_str()),
        )
    })?;
    cleanup_committed(&state.auth, &mut node, &origins);
    Ok(StatusCode::NO_CONTENT)
}

// Credential revocation has already committed. Reporting failure here would falsely
// imply the credential still works; durable orphan scanning retries runtime cleanup.
pub(super) fn cleanup_committed<P: JudgmentProvider>(
    auth: &Auth,
    node: &mut babble_node::LocalNode<P>,
    origins: &[String],
) {
    for origin in origins {
        if surface::cleanup_origin_locked(auth, node, origin).is_err() {
            eprintln!("Account revocation committed; Surface cleanup pending retry");
        }
    }
}
