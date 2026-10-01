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
            "babble-safety-http-{}-{}-{}",
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
    if uri.starts_with("/social/safety") || status.is_success() {
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
        json!({"handle":handle,"kind":"Person","password":"Durable Safety account 930403!"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Account {
        id: body["identity"]["id"].as_str().unwrap().into(),
        token: body["token"].as_str().unwrap().into(),
    }
}
fn target(account: &Account) -> String {
    format!("/social/safety/{}", account.id)
}

fn mutation(blocked: bool, muted: bool, revision: u64, key: &str) -> Value {
    json!({"blocked":blocked,"muted":muted,"expected_revision":revision,"idempotency_key":key})
}

async fn rpc(
    app: &Router,
    actor: &Account,
    method: &str,
    payload: Value,
    key: &str,
) -> (StatusCode, Value) {
    let envelope = babble_rpc::RpcRequestEnvelope::new(
        &babble_rpc::babble_rpc_catalog().unwrap(),
        key,
        method,
        babble_rpc::RpcBinding::host("safety-tests", "https://babble.test").unwrap(),
        payload,
    )
    .unwrap()
    .with_idempotency_key(key);
    request(app, "POST", "/rpc", Some(&actor.token), json!(envelope)).await
}

#[tokio::test]
async fn safety_http_and_rpc_mutation_paths_enforce_both_directions_and_preserve_public_reads() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let mut posts = Vec::new();
    for actor in [&alice, &bob] {
        let (status, result) = request(
            &app,
            "POST",
            "/objects/text",
            Some(&actor.token),
            json!({"author_id":actor.id,"text":"post"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        posts.push(result["object"]["id"].as_str().unwrap().to_owned());
    }
    request(
        &app,
        "PUT",
        &format!("/social/following/{}", bob.id),
        Some(&alice.token),
        json!({"following":true,"expected_revision":0,"idempotency_key":"follow"}),
    )
    .await;
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(true, false, 0, "block")
        )
        .await
        .0,
        StatusCode::OK
    );
    for (actor, other, source, target) in [
        (&alice, &bob, &posts[0], &posts[1]),
        (&bob, &alice, &posts[1], &posts[0]),
    ] {
        let edge = json!({"author_id":actor.id,"source":source,"target":target,"relation":"reply_to","origin":"HumanAssertion"});
        let result = request(
            &app,
            "POST",
            "/graph/edges",
            Some(&actor.token),
            edge.clone(),
        )
        .await;
        assert_eq!(result.0, StatusCode::CONFLICT, "{result:?}");
        let result = rpc(&app, actor, "babble.graph.edge.publish.v1", edge, "edge").await;
        assert_eq!(result.1["error"]["code"], "CONFLICT", "{result:?}");
        assert!(
            result.1.to_string().contains("interaction unavailable"),
            "{result:?}"
        );
        let draft = babble_authoring::ObjectDraft::text("generic provenance")
            .unwrap()
            .with_provenance(babble_object::Provenance {
                parent: Some(babble_types::ObjectId::new_unchecked(target.clone())),
                forked_from: None,
                remixed_from: vec![],
            })
            .unwrap();
        let payload = json!({"author_id":actor.id,"draft":draft});
        assert_eq!(
            request(
                &app,
                "POST",
                "/objects",
                Some(&actor.token),
                payload.clone()
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        let result = rpc(&app, actor, "babble.object.publish.v1", payload, "generic").await;
        assert!(
            result.1.to_string().contains("interaction unavailable"),
            "{result:?}"
        );
        let value = json!({"appreciation":"like","engagement":null,"stance":null,"certainty":null});
        let reaction = json!({"value":value,"expected_revision":0,"idempotency_key":"reaction"});
        let result = request(
            &app,
            "PUT",
            &format!("/objects/{target}/reactions/mine"),
            Some(&actor.token),
            reaction,
        )
        .await;
        assert_eq!(result.0, StatusCode::CONFLICT, "{result:?}");
        let result = rpc(
            &app,
            actor,
            "babble.social.reactions.set.v1",
            json!({"object_id":target,"value":value,"expected_revision":0}),
            "reaction-rpc",
        )
        .await;
        assert!(
            result.1.to_string().contains("interaction unavailable"),
            "{result:?}"
        );
        assert_eq!(request(&app, "PUT", &format!("/social/following/{}", other.id), Some(&actor.token),
            json!({"following":true,"expected_revision":if actor.id == alice.id {1} else {0},"idempotency_key":"blocked-follow"})).await.0, StatusCode::CONFLICT);
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("/objects/{target}"),
                None,
                Value::Null
            )
            .await
            .0,
            StatusCode::OK
        );
    }
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
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(false, true, 1, "unblock")
        )
        .await
        .0,
        StatusCode::OK
    );
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
    assert_eq!(request(&app, "POST", "/graph/edges", Some(&alice.token),
        json!({"author_id":alice.id,"source":posts[0],"target":posts[1],"relation":"reply_to","origin":"HumanAssertion"})).await.0, StatusCode::OK);
}

#[tokio::test]
async fn safety_http_auth_owner_isolation_validation_restart_and_exact_retry() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    for uri in ["/social/safety", &target(&bob)] {
        assert_eq!(
            request(&app, "GET", uri, None, Value::Null).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            None,
            mutation(true, true, 0, "on")
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, on) = request(
        &app,
        "PUT",
        &target(&bob),
        Some(&alice.token),
        mutation(true, true, 0, "on"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{on}");
    assert_eq!(
        on,
        json!({"author_id":alice.id,"target_id":bob.id,"blocked":true,"muted":true,"revision":1})
    );
    let (_, snapshot) = request(
        &app,
        "GET",
        "/social/safety",
        Some(&alice.token),
        Value::Null,
    )
    .await;
    assert_eq!(snapshot["author_id"], alice.id);
    assert_eq!(snapshot["revision"], 1);
    assert_eq!(snapshot["entries"][0]["identity"]["id"], bob.id);
    assert_eq!(snapshot["entries"][0]["state"], on);
    assert_eq!(
        request(&app, "GET", "/social/safety", Some(&bob.token), Value::Null)
            .await
            .1["entries"],
        json!([])
    );
    assert_eq!(
        request(&app, "GET", &target(&alice), Some(&bob.token), Value::Null)
            .await
            .1["revision"],
        0
    );
    for body in [
        json!({"blocked":true,"muted":false,"expected_revision":0,"idempotency_key":"forged","author_id":bob.id}),
        json!({"blocked":true,"expected_revision":0,"idempotency_key":"missing"}),
        json!({"blocked":"true","muted":false,"expected_revision":0,"idempotency_key":"bad-type"}),
        mutation(false, false, u64::MAX, "range"),
        mutation(false, false, 1, ""),
        mutation(false, false, 1, "bad key"),
        mutation(false, false, 1, &"x".repeat(257)),
    ] {
        assert!(
            request(&app, "PUT", &target(&bob), Some(&alice.token), body)
                .await
                .0
                .is_client_error()
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(false, false, 0, "stale")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(false, true, 1, "on")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, muted) = request(
        &app,
        "PUT",
        &target(&bob),
        Some(&alice.token),
        mutation(false, true, 1, "unblock"),
    )
    .await;
    assert_eq!(muted["muted"], true);
    assert_eq!(muted["blocked"], false);
    drop(app);
    let app = f.app();
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(true, true, 0, "on")
        )
        .await
        .1,
        on
    );
    assert_eq!(
        request(&app, "GET", &target(&bob), Some(&alice.token), Value::Null)
            .await
            .1,
        muted
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(false, false, 2, "clear")
        )
        .await
        .1["revision"],
        3
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/social/safety",
            Some(&alice.token),
            Value::Null
        )
        .await
        .1["entries"],
        json!([])
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/social/safety")
                .header("authorization", format!("Bearer {}", alice.token))
                .header("x-babble-surface-document", "embedded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_client_error());
    request(
        &app,
        "DELETE",
        "/auth/session",
        Some(&alice.token),
        Value::Null,
    )
    .await;
    assert_eq!(
        request(
            &app,
            "GET",
            "/social/safety",
            Some(&alice.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn safety_http_storage_failure_does_not_consume_request_key() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let database = f.0.join("private_safety/safety.sqlite3");
    let backup = database.with_extension("backup");
    fs::rename(&database, &backup).unwrap();
    let (status, body) = request(
        &app,
        "PUT",
        &target(&bob),
        Some(&alice.token),
        mutation(true, false, 0, "retry"),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(!database.exists());
    fs::rename(backup, database).unwrap();
    assert_eq!(
        request(
            &app,
            "PUT",
            &target(&bob),
            Some(&alice.token),
            mutation(true, false, 0, "retry")
        )
        .await
        .1["revision"],
        1
    );
}
