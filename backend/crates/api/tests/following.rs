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

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "babble-follow-http-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn app(&self) -> Router {
        router(ApiState::new(
            LocalNode::open(&self.0, LocalProvider::default()).unwrap(),
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

struct Account {
    id: String,
    token: String,
}
async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    value: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let body = if value.is_null() {
        Body::empty()
    } else {
        builder = builder.header("content-type", "application/json");
        Body::from(serde_json::to_vec(&value).unwrap())
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    if status.is_success() {
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn register(app: &Router, handle: &str) -> Account {
    let (status, body) = request(
        app,
        "POST",
        "/auth/register",
        None,
        json!({"handle":handle,"kind":"Person","password":"Durable Following account 930403!"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Account {
        id: body["identity"]["id"].as_str().unwrap().into(),
        token: body["token"].as_str().unwrap().into(),
    }
}
fn target(account: &Account) -> String {
    format!("/social/following/{}", account.id)
}
fn mutation(following: bool, revision: u64, key: &str) -> Value {
    json!({"following":following,"expected_revision":revision,"idempotency_key":key})
}
async fn set(
    app: &Router,
    actor: &Account,
    other: &Account,
    following: bool,
    revision: u64,
    key: &str,
) -> Value {
    let (status, body) = request(
        app,
        "PUT",
        &target(other),
        Some(&actor.token),
        mutation(following, revision, key),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}
async fn publish(app: &Router, actor: &Account, text: &str) -> babble_object::Object {
    let (status, body) = request(
        app,
        "POST",
        "/objects/text",
        Some(&actor.token),
        json!({"author_id":actor.id,"text":text}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_value(body["object"].clone()).unwrap()
}
fn cursor_url(path: &str, cursor: &Value, limit: usize) -> String {
    let mut url = reqwest::Url::parse(&format!("http://babble.test{path}")).unwrap();
    url.query_pairs_mut()
        .append_pair("cursor", cursor.as_str().unwrap())
        .append_pair("limit", &limit.to_string());
    format!("{}?{}", url.path(), url.query().unwrap())
}

#[tokio::test]
async fn following_http_is_private_and_never_accepts_an_actor_override() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let carol = register(&app, "carol").await;
    for uri in ["/social/following", "/feed/following", &target(&bob)] {
        assert_eq!(
            request(&app, "GET", uri, None, Value::Null).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        request(&app, "PUT", &target(&bob), None, mutation(true, 0, "guest"))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    for field in ["author_id", "identity_id"] {
        let mut body = mutation(true, 0, "forged");
        body[field] = json!(alice.id);
        assert_eq!(
            request(&app, "PUT", &target(&bob), Some(&carol.token), body)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let state = set(&app, &alice, &bob, true, 0, "follow-bob").await;
    assert_eq!(
        state,
        json!({"author_id":alice.id,"target_id":bob.id,"following":true,"revision":1})
    );
    let (status, other) =
        request(&app, "GET", &target(&bob), Some(&carol.token), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(other["following"], false);
    for uri in [
        format!("/social/following?identity_id={}", alice.id),
        format!("/feed/following?author_id={}", alice.id),
    ] {
        assert!(
            request(&app, "GET", &uri, Some(&carol.token), Value::Null)
                .await
                .0
                .is_client_error()
        );
    }
    let (_, list) = request(
        &app,
        "GET",
        "/social/following",
        Some(&carol.token),
        Value::Null,
    )
    .await;
    assert_eq!(list["identities"], json!([]));
    assert!(
        request(
            &app,
            "PUT",
            &target(&alice),
            Some(&alice.token),
            mutation(true, 0, "self")
        )
        .await
        .0
        .is_client_error()
    );
    request(
        &app,
        "DELETE",
        "/auth/session",
        Some(&alice.token),
        Value::Null,
    )
    .await;
    assert_eq!(
        request(&app, "GET", &target(&bob), Some(&alice.token), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(false, 1, "logout")
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn following_http_restart_retry_and_stale_write_preserve_current_state() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let first = set(&app, &alice, &bob, true, 0, "intent-1").await;
    drop(app);
    let app = f.app();
    assert_eq!(set(&app, &alice, &bob, true, 0, "intent-1").await, first);
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(false, 0, "stale")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let second = set(&app, &alice, &bob, false, 1, "intent-2").await;
    assert_eq!(second["revision"], 2);
    assert_eq!(set(&app, &alice, &bob, true, 0, "intent-1").await, first);
    let (_, current) = request(&app, "GET", &target(&bob), Some(&alice.token), Value::Null).await;
    assert_eq!(current, second, "old retry must not undo a later unfollow");
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(false, 2, "intent-1")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    drop(app);
    let app = f.app();
    assert_eq!(
        request(&app, "GET", &target(&bob), Some(&alice.token), Value::Null)
            .await
            .1,
        second
    );
    assert!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(true, 2, &"x".repeat(257))
        )
        .await
        .0
        .is_client_error()
    );
}

#[tokio::test]
async fn following_http_feed_is_chronological_paginated_and_only_followed_authors() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let carol = register(&app, "carol").await;
    let mut objects = Vec::new();
    for text in [
        "first followed post",
        "second followed post",
        "last followed post",
    ] {
        objects.push(publish(&app, &bob, text).await);
    }
    publish(&app, &alice, "own post not a followed person").await;
    publish(&app, &carol, "unfollowed post must not leak into Following").await;
    assert_eq!(
        request(
            &app,
            "GET",
            "/feed/following",
            Some(&alice.token),
            Value::Null
        )
        .await
        .1["objects"],
        json!([])
    );
    set(&app, &alice, &bob, true, 0, "follow").await;
    objects.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    let (_, first) = request(
        &app,
        "GET",
        "/feed/following?limit=2",
        Some(&alice.token),
        Value::Null,
    )
    .await;
    assert_eq!(first["objects"], json!(objects[..2]));
    let uri = cursor_url("/feed/following", &first["next_cursor"], 2);
    let (_, last) = request(&app, "GET", &uri, Some(&alice.token), Value::Null).await;
    assert_eq!(last["objects"], json!(objects[2..]));
    assert!(last["next_cursor"].is_null());
    let (_, searched) = request(
        &app,
        "GET",
        "/feed/following?search=second",
        Some(&alice.token),
        Value::Null,
    )
    .await;
    assert_eq!(searched["objects"], json!([objects[1]]));
    assert!(
        request(&app, "GET", &uri, Some(&carol.token), Value::Null)
            .await
            .0
            .is_client_error()
    );
    publish(&app, &bob, "new post invalidates pagination snapshot").await;
    assert_eq!(
        request(&app, "GET", &uri, Some(&alice.token), Value::Null)
            .await
            .0,
        StatusCode::CONFLICT
    );
    set(&app, &alice, &bob, false, 1, "unfollow").await;
    assert_eq!(
        request(
            &app,
            "GET",
            "/feed/following",
            Some(&alice.token),
            Value::Null
        )
        .await
        .1["objects"],
        json!([])
    );
    for suffix in [
        "?limit=0",
        "?limit=51",
        "?cursor=garbage",
        "?unexpected=1",
        "?search=bad&search=duplicate",
    ] {
        assert!(
            request(
                &app,
                "GET",
                &format!("/feed/following{suffix}"),
                Some(&alice.token),
                Value::Null
            )
            .await
            .0
            .is_client_error(),
            "{suffix}"
        );
    }
}

#[tokio::test]
async fn following_http_list_is_paged_and_follow_changes_invalidate_cursors() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let carol = register(&app, "carol").await;
    set(&app, &alice, &bob, true, 0, "bob").await;
    set(&app, &alice, &carol, true, 0, "carol").await;
    let (_, first) = request(
        &app,
        "GET",
        "/social/following?limit=1",
        Some(&alice.token),
        Value::Null,
    )
    .await;
    assert_eq!(first["identities"].as_array().unwrap().len(), 1);
    let uri = cursor_url("/social/following", &first["next_cursor"], 1);
    let (_, last) = request(&app, "GET", &uri, Some(&alice.token), Value::Null).await;
    assert_eq!(last["identities"].as_array().unwrap().len(), 1);
    assert_ne!(first["identities"][0]["id"], last["identities"][0]["id"]);
    assert!(last["next_cursor"].is_null());
    set(&app, &alice, &bob, false, 1, "unfollow").await;
    assert_eq!(
        request(&app, "GET", &uri, Some(&alice.token), Value::Null)
            .await
            .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn following_http_storage_failure_is_retryable_not_a_cas_conflict() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let database = f.0.join("private_following/following.sqlite3");
    let backup = database.with_extension("backup");
    fs::rename(&database, &backup).unwrap();
    fs::create_dir(&database).unwrap();
    let (status, body) = request(
        &app,
        "PUT",
        &target(&bob),
        Some(&alice.token),
        mutation(true, 0, "storage-retry"),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "storage_unavailable");
    fs::remove_dir(&database).unwrap();
    fs::rename(backup, database).unwrap();
    let state = set(&app, &alice, &bob, true, 0, "storage-retry").await;
    assert_eq!(state["revision"], 1);
    assert_eq!(
        set(&app, &alice, &bob, true, 0, "storage-retry").await,
        state
    );
}
