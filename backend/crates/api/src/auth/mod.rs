pub(crate) mod policy;
mod security;
mod store;
pub(crate) mod surface;
pub use security::{AccountSessionInfo, AccountSessionsResponse, ChangePasswordRequest};
pub(crate) use surface::authorize_bundle;
pub(crate) use surface::check_execution;
pub(crate) use surface::heartbeat_surface;
pub(crate) use surface::start_surface;
pub(crate) use surface::{DOCUMENT_HEADER, bind_surface_document};
#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod surface_tests;

use crate::{ApiState, error::ApiError, routes::lock_node};
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    routing::post,
};
use babble_identity::{Identity, IdentityKind};
use babble_judgment::JudgmentProvider;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use store::AuthStore;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

#[derive(Clone)]
pub(crate) struct HttpBoundary;

#[derive(Clone)]
pub(crate) struct Principal {
    pub identity_id: String,
    pub expires_at: i64,
    pub account_session: String,
}

pub(crate) struct Auth {
    store: Mutex<Result<AuthStore, ApiError>>,
    work: Arc<Semaphore>,
    attempts: Mutex<(Instant, u32)>,
    operator_hash: Option<String>,
    cleanup_started: AtomicBool,
}

impl Auth {
    pub fn new(root: &Path) -> Self {
        Self {
            store: Mutex::new(AuthStore::open(root)),
            work: Arc::new(Semaphore::new(4)),
            attempts: Mutex::new((Instant::now(), 0)),
            operator_hash: None,
            cleanup_started: AtomicBool::new(false),
        }
    }

    pub fn with_operator(mut self, token: Option<&str>) -> Self {
        self.operator_hash = token.filter(|value| value.len() >= 32).map(store::digest);
        self
    }

    pub fn check_ready(&self) -> Result<(), ApiError> {
        self.with_store(|_| Ok(()))
    }

    pub(crate) fn retire_evicted_surface(
        &self,
        session: &babble_runtime::SurfaceSession,
    ) -> Result<(), ApiError> {
        if session.lifecycle == babble_runtime::SurfaceLifecycle::Evicted {
            self.with_store(|store| store.retire_surface(session.id.as_str()))?;
        }
        Ok(())
    }

    pub(crate) fn with_store<T>(
        &self,
        f: impl FnOnce(&mut AuthStore) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| ApiError::unavailable("account store lock unavailable"))?;
        let store = store
            .as_mut()
            .map_err(|_| ApiError::unavailable("account store unavailable"))?;
        f(store)
    }

    pub fn principal(&self, headers: &HeaderMap) -> Result<Principal, ApiError> {
        self.with_store(|store| store.authenticate(bearer(headers)?))
    }

    pub fn operator(&self, headers: &HeaderMap) -> Result<(), ApiError> {
        let supplied = store::digest(bearer(headers)?);
        let expected = self
            .operator_hash
            .as_deref()
            .ok_or_else(ApiError::forbidden)?;
        let difference = supplied
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b));
        if difference == 0 {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }

    fn password_slot(&self) -> Result<OwnedSemaphorePermit, ApiError> {
        let permit = self
            .work
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::rate_limited())?;
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| ApiError::internal("authentication limiter unavailable"))?;
        if attempts.0.elapsed() >= Duration::from_secs(60) {
            *attempts = (Instant::now(), 0);
        }
        if attempts.1 >= 60 {
            return Err(ApiError::rate_limited());
        }
        attempts.1 += 1;
        Ok(permit)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    handle: String,
    kind: IdentityKind,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    identity_id: String,
    password: String,
}

#[derive(Serialize)]
struct AccountResponse {
    identity: Identity,
    token: String,
    expires_at: String,
}

#[derive(Serialize)]
struct SessionResponse {
    identity: Identity,
    expires_at: String,
}

pub(crate) fn router<P: JudgmentProvider + Send + Sync + 'static>(state: ApiState<P>) -> Router {
    surface::start_cleanup(&state);
    Router::new()
        .route("/auth/register", post(register::<P>))
        .route("/auth/login", post(login::<P>))
        .route(
            "/auth/sessions",
            axum::routing::get(security::sessions::<P>),
        )
        .route(
            "/auth/sessions/{id}",
            axum::routing::delete(security::revoke_session::<P>),
        )
        .route(
            "/auth/sessions/revoke-others",
            post(security::revoke_others::<P>),
        )
        .route("/auth/password", post(security::change_password::<P>))
        .route(
            "/auth/session",
            axum::routing::get(session::<P>).delete(logout::<P>),
        )
        .layer(axum::middleware::from_fn(no_store))
        .with_state(state)
}

async fn no_store(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

fn password_engine() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19456, 2, 1, Some(32)).expect("fixed valid Argon2 parameters"),
    )
}

fn password_policy(password: &str) -> Result<(), ApiError> {
    if password.len() > 1024 || password.chars().count() < 15 {
        return Err(ApiError::bad_request(
            "password must contain at least 15 Unicode characters and at most 1024 UTF-8 bytes",
        ));
    }
    Ok(())
}

async fn register<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Json(input): Json<RegisterRequest>,
) -> Result<Json<AccountResponse>, ApiError> {
    let password = Zeroizing::new(input.password);
    password_policy(&password)?;
    let handle = input.handle.trim();
    if handle.is_empty() || handle.len() > 128 || handle.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            "handle must contain 1 to 128 bytes without control characters",
        ));
    }
    let permit = state.auth.password_slot()?;
    let hash = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let salt = SaltString::generate(&mut OsRng);
        password_engine()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| Zeroizing::new(hash.to_string()))
            .map_err(|_| ApiError::internal("password hashing failed"))
    })
    .await
    .map_err(|_| ApiError::internal("password worker failed"))??;
    let identity = lock_node(&state)?.create_identity(input.kind, handle)?;
    state
        .auth
        .with_store(|store| store.create_account(identity.id.as_str(), &hash))?;
    issue(&state, identity, &hash)
}

async fn login<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<AccountResponse>, ApiError> {
    let password = Zeroizing::new(input.password);
    if password.len() > 1024 || input.identity_id.len() > 128 {
        return Err(ApiError::unauthorized());
    }
    let permit = state.auth.password_slot()?;
    let hash = state
        .auth
        .with_store(|store| store.password_hash(&input.identity_id))?
        .map(Zeroizing::new);
    let (valid, hash) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let valid = match hash.as_ref() {
            Some(hash) => PasswordHash::new(hash).ok().is_some_and(|hash| {
                password_engine()
                    .verify_password(password.as_bytes(), &hash)
                    .is_ok()
            }),
            None => {
                // Unknown accounts consume the same expensive password work.
                let salt = SaltString::generate(&mut OsRng);
                let _ = password_engine().hash_password(password.as_bytes(), &salt);
                false
            }
        };
        (valid, hash)
    })
    .await
    .map_err(|_| ApiError::internal("password worker failed"))?;
    if !valid {
        return Err(ApiError::unauthorized());
    }
    let identity = lock_node(&state)?
        .identity(&babble_types::IdentityId::new_unchecked(input.identity_id))
        .cloned()
        .ok_or_else(ApiError::unauthorized)?;
    issue(
        &state,
        identity,
        hash.as_deref().ok_or_else(ApiError::unauthorized)?,
    )
}

fn issue<P: JudgmentProvider>(
    state: &ApiState<P>,
    identity: Identity,
    observed_hash: &str,
) -> Result<Json<AccountResponse>, ApiError> {
    let _node = lock_node(state)?;
    let (token, expires) = state
        .auth
        .with_store(|store| store.issue_verified(identity.id.as_str(), observed_hash))?;
    Ok(Json(AccountResponse {
        identity,
        token,
        expires_at: format_expiry(expires)?,
    }))
}

async fn session<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
) -> Result<Json<SessionResponse>, ApiError> {
    let node = lock_node(&state)?;
    let principal = state.auth.principal(&headers)?;
    let identity = node
        .identity(&babble_types::IdentityId::new_unchecked(
            principal.identity_id,
        ))
        .cloned()
        .ok_or_else(ApiError::unauthorized)?;
    Ok(Json(SessionResponse {
        identity,
        expires_at: format_expiry(principal.expires_at)?,
    }))
}

async fn logout<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let mut node = lock_node(&state)?;
    let principal = state.auth.principal(&headers)?;
    state
        .auth
        .with_store(|store| store.revoke(bearer(&headers)?))?;
    security::cleanup_committed(&state.auth, &mut node, &[principal.account_session]);
    Ok(StatusCode::NO_CONTENT)
}

fn format_expiry(value: i64) -> Result<String, ApiError> {
    OffsetDateTime::from_unix_timestamp(value)
        .map_err(|_| ApiError::internal("invalid session expiry"))?
        .format(&Rfc3339)
        .map_err(|_| ApiError::internal("invalid session expiry"))
}

fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next().ok_or_else(ApiError::unauthorized)?;
    if values.next().is_some() {
        return Err(ApiError::unauthorized());
    }
    let value = value.to_str().map_err(|_| ApiError::unauthorized())?;
    let (scheme, token) = value.split_once(' ').ok_or_else(ApiError::unauthorized)?;
    if !scheme.eq_ignore_ascii_case("bearer")
        || token.is_empty()
        || token.bytes().any(|b| b.is_ascii_whitespace())
    {
        return Err(ApiError::unauthorized());
    }
    Ok(token)
}

pub(crate) fn random_token() -> Result<String, ApiError> {
    let mut bytes = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| ApiError::unavailable("secure randomness unavailable"))?;
    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_password_capacity_and_attempt_budget_are_bounded() {
        let auth = Auth {
            store: Mutex::new(Err(ApiError::internal("unused"))),
            work: Arc::new(Semaphore::new(4)),
            attempts: Mutex::new((Instant::now(), 0)),
            operator_hash: None,
            cleanup_started: AtomicBool::new(false),
        };
        let permits: Vec<_> = (0..4).map(|_| auth.password_slot().unwrap()).collect();
        assert!(auth.password_slot().is_err());
        drop(permits);
        for _ in 4..60 {
            drop(auth.password_slot().unwrap());
        }
        assert!(auth.password_slot().is_err());
        *auth.attempts.lock().unwrap() = (Instant::now() - Duration::from_secs(61), 60);
        assert!(auth.password_slot().is_ok());
    }

    #[test]
    fn auth_operator_credential_is_separate_and_duplicate_headers_fail_closed() {
        let token = "test-operator-secret-with-more-than-thirty-two-bytes";
        let auth = Auth {
            store: Mutex::new(Err(ApiError::internal("unavailable"))),
            work: Arc::new(Semaphore::new(4)),
            attempts: Mutex::new((Instant::now(), 0)),
            operator_hash: None,
            cleanup_started: AtomicBool::new(false),
        }
        .with_operator(Some(token));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert!(auth.operator(&headers).is_ok());
        assert!(auth.principal(&headers).is_err());
        headers.append(
            header::AUTHORIZATION,
            "Bearer another-token".parse().unwrap(),
        );
        assert!(auth.operator(&headers).is_err());
        assert!(auth.check_ready().is_err());
    }
}
