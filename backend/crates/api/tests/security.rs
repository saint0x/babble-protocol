use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babble_api::{ApiState, router};
use babble_judgment_local::LocalProvider;
use babble_node::LocalNode;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tower::ServiceExt;

const PASSWORD: &str = "correct horse durable session";
const REPLACEMENT: &str = "different horse durable password";

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        Self {
            root: std::env::temp_dir().join(format!(
                "babble-security-{}-{}-{}",
                std::process::id(),
                time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            )),
        }
    }
    fn state(&self) -> ApiState<LocalProvider> {
        ApiState::new(LocalNode::open(&self.root, LocalProvider::default()).unwrap())
    }
    fn app(&self) -> Router {
        router(self.state())
    }
    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.join("auth/accounts.sqlite3")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone)]
struct Account {
    id: String,
    token: String,
}

async fn raw(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, Value) {
    raw_with_headers(app, method, uri, token, body, &[]).await
}

async fn raw_with_headers(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<&str>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(body.map_or_else(Body::empty, |body| Body::from(body.to_owned())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    if uri.starts_with("/auth/") {
        assert_eq!(
            response
                .headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "{method} {uri}: {status}"
        );
    }
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, body)
}
async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    raw(
        app,
        method,
        uri,
        token,
        (!body.is_null()).then(|| body.to_string()).as_deref(),
    )
    .await
}
async fn register_with(app: &Router, password: &str) -> Account {
    let (status, result) = request(
        app,
        "POST",
        "/auth/register",
        None,
        json!({"handle":"security-test", "kind":"Person", "password":password}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    Account {
        id: result["identity"]["id"].as_str().unwrap().into(),
        token: result["token"].as_str().unwrap().into(),
    }
}
async fn register(app: &Router) -> Account {
    register_with(app, PASSWORD).await
}
async fn login(app: &Router, account: &Account, password: &str) -> Account {
    let (status, result) = request(
        app,
        "POST",
        "/auth/login",
        None,
        json!({"identity_id":account.id,"password":password}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["identity"]["id"], account.id);
    Account {
        id: account.id.clone(),
        token: result["token"].as_str().unwrap().into(),
    }
}
async fn sessions(app: &Router, account: &Account) -> Vec<Value> {
    let (status, result) = request(
        app,
        "GET",
        "/auth/sessions",
        Some(&account.token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result.as_object().unwrap().len(), 1);
    let sessions = result["sessions"].as_array().unwrap().clone();
    assert!(sessions.len() <= 16);
    assert_eq!(sessions.iter().filter(|s| s["current"] == true).count(), 1);
    for session in &sessions {
        let keys: std::collections::BTreeSet<_> = session
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["id", "created_at", "expires_at", "current"]
                .into_iter()
                .collect()
        );
        let id = session["id"].as_str().unwrap();
        let suffix = id
            .strip_prefix("account_")
            .expect("independent account session identifier");
        assert_eq!(suffix.len(), 64);
        assert!(
            suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert!(session["current"].is_boolean());
        let expires = parse_time(&session["expires_at"]);
        assert!(expires > time::OffsetDateTime::now_utc());
        if !session["created_at"].is_null() {
            assert!(parse_time(&session["created_at"]) <= expires);
        }
    }
    sessions
}
fn parse_time(value: &Value) -> time::OffsetDateTime {
    time::OffsetDateTime::parse(
        value.as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap()
}
fn current_id(sessions: &[Value]) -> String {
    sessions.iter().find(|s| s["current"] == true).unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
async fn status(app: &Router, account: &Account, expected: StatusCode) {
    assert_eq!(
        request(
            app,
            "GET",
            "/auth/session",
            Some(&account.token),
            Value::Null
        )
        .await
        .0,
        expected
    );
}
async fn no_content(app: &Router, method: &str, uri: &str, account: &Account, body: Value) {
    let result = request(app, method, uri, Some(&account.token), body).await;
    assert_eq!(result, (StatusCode::NO_CONTENT, Value::Null));
}
async fn change(app: &Router, account: &Account, old: &str, new: &str) -> (StatusCode, Value) {
    request(
        app,
        "POST",
        "/auth/password",
        Some(&account.token),
        json!({"current_password":old,"new_password":new}),
    )
    .await
}

#[tokio::test]
async fn security_sessions_are_private_stable_independent_and_not_credentials() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app).await;
    let second = login(&app, &alice, PASSWORD).await;
    let bob = register(&app).await;
    let list = sessions(&app, &alice).await;
    assert_eq!(list.len(), 2);
    assert!(list.iter().all(|s| !s["created_at"].is_null()));
    assert_eq!(sessions(&app, &bob).await.len(), 1);
    let own_id = current_id(&list);
    let second_id = current_id(&sessions(&app, &second).await);
    assert_ne!(own_id, second_id);
    let serialized = serde_json::to_string(&list).unwrap();
    for secret in [&alice.token, &second.token, &bob.token] {
        let hash = blake3::hash(secret.as_bytes()).to_hex().to_string();
        assert!(!serialized.contains(secret));
        assert!(!serialized.contains(&hash));
    }
    for forged in [own_id.as_str(), own_id.strip_prefix("account_").unwrap()] {
        assert_eq!(
            request(&app, "GET", "/auth/session", Some(forged), Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    drop(app);
    let app = fixture.app();
    assert_eq!(sessions(&app, &alice).await, list);
    assert_eq!(current_id(&sessions(&app, &second).await), second_id);
}

#[tokio::test]
async fn security_revoke_one_is_idempotent_owner_scoped_and_persistent_including_self() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app).await;
    let second = login(&app, &alice, PASSWORD).await;
    let bob = register(&app).await;
    let first_id = current_id(&sessions(&app, &alice).await);
    let second_id = current_id(&sessions(&app, &second).await);
    let bob_id = current_id(&sessions(&app, &bob).await);
    for id in [
        &bob_id,
        &format!("account_{}", "0".repeat(64)),
        &second_id,
        &second_id,
    ] {
        no_content(
            &app,
            "DELETE",
            &format!("/auth/sessions/{id}"),
            &alice,
            Value::Null,
        )
        .await;
    }
    status(&app, &alice, StatusCode::OK).await;
    status(&app, &second, StatusCode::UNAUTHORIZED).await;
    status(&app, &bob, StatusCode::OK).await;
    no_content(
        &app,
        "DELETE",
        &format!("/auth/sessions/{first_id}"),
        &alice,
        Value::Null,
    )
    .await;
    status(&app, &alice, StatusCode::UNAUTHORIZED).await;
    drop(app);
    let app = fixture.app();
    for account in [&alice, &second] {
        status(&app, account, StatusCode::UNAUTHORIZED).await;
    }
    status(&app, &bob, StatusCode::OK).await;
    let fresh = login(&app, &alice, PASSWORD).await;
    assert_eq!(sessions(&app, &fresh).await.len(), 1);
}

#[tokio::test]
async fn security_revoke_others_keeps_only_actor_and_cannot_target_another_account() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app).await;
    let second = login(&app, &alice, PASSWORD).await;
    let third = login(&app, &alice, PASSWORD).await;
    let bob = register(&app).await;
    no_content(
        &app,
        "POST",
        "/auth/sessions/revoke-others",
        &second,
        Value::Null,
    )
    .await;
    no_content(
        &app,
        "POST",
        "/auth/sessions/revoke-others",
        &second,
        Value::Null,
    )
    .await;
    status(&app, &alice, StatusCode::UNAUTHORIZED).await;
    status(&app, &third, StatusCode::UNAUTHORIZED).await;
    status(&app, &bob, StatusCode::OK).await;
    assert_eq!(sessions(&app, &second).await.len(), 1);
    drop(app);
    let app = fixture.app();
    for account in [&alice, &third] {
        status(&app, account, StatusCode::UNAUTHORIZED).await;
    }
    status(&app, &second, StatusCode::OK).await;
    status(&app, &bob, StatusCode::OK).await;
}

#[tokio::test]
async fn security_password_change_revokes_every_device_and_retry_is_unauthorized() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app).await;
    let second = login(&app, &alice, PASSWORD).await;
    let bob = register(&app).await;
    assert_eq!(
        change(&app, &alice, "incorrect password", REPLACEMENT)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    status(&app, &alice, StatusCode::OK).await;
    status(&app, &second, StatusCode::OK).await;
    assert_eq!(
        change(&app, &alice, PASSWORD, REPLACEMENT).await,
        (StatusCode::NO_CONTENT, Value::Null)
    );
    assert_eq!(
        change(&app, &alice, PASSWORD, REPLACEMENT).await.0,
        StatusCode::UNAUTHORIZED
    );
    for account in [&alice, &second] {
        status(&app, account, StatusCode::UNAUTHORIZED).await;
        assert_eq!(
            request(
                &app,
                "GET",
                "/social/following",
                Some(&account.token),
                Value::Null
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                "POST",
                "/objects/text",
                Some(&account.token),
                json!({"author_id":alice.id,"text":"revoked write"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    status(&app, &bob, StatusCode::OK).await;
    drop(app);
    let app = fixture.app();
    for account in [&alice, &second] {
        status(&app, account, StatusCode::UNAUTHORIZED).await;
    }
    assert_eq!(
        request(
            &app,
            "POST",
            "/auth/login",
            None,
            json!({"identity_id":alice.id,"password":PASSWORD})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let fresh = login(&app, &alice, REPLACEMENT).await;
    assert_eq!(sessions(&app, &fresh).await.len(), 1);
    let hash: String = fixture
        .db()
        .query_row(
            "SELECT password_hash FROM accounts WHERE identity_id=?1",
            [&alice.id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    status(&app, &bob, StatusCode::OK).await;
}

#[path = "security/persistence.rs"]
mod persistence;
#[path = "security/surfaces.rs"]
mod surfaces;
#[path = "security/validation.rs"]
mod validation;
