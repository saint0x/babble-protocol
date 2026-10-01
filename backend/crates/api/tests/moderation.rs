use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babel_api::{ApiState, router};
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tower::ServiceExt;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "babel-moderation-http-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn app(&self, reviewers: &str) -> Router {
        router(
            ApiState::new(LocalNode::open(&self.0, LocalProvider::default()).unwrap())
                .with_moderators(reviewers)
                .unwrap(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Account {
    id: String,
    token: String,
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    account: Option<&Account>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(account) = account {
        request = request.header("authorization", format!("Bearer {}", account.token));
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    if path.starts_with("/moderation/") {
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let bytes = to_bytes(response.into_body(), 4_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn register(app: &Router, name: &str) -> Account {
    let (status, body) = call(
        app,
        "POST",
        "/auth/register",
        None,
        json!({"handle":name,"kind":"Person","password":"Moderation durable password 930!"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Account {
        id: body["identity"]["id"].as_str().unwrap().into(),
        token: body["token"].as_str().unwrap().into(),
    }
}
fn report(object: &str, key: &str) -> Value {
    json!({"object_id":object,"reason":"fraud","details":"Reporter private evidence sufficiently detailed","idempotency_key":key})
}
fn decision(outcome: &str, revision: u64, key: &str) -> Value {
    json!({"outcome":outcome,"reason":"fraud","explanation":"Reviewer explained the integrity policy decision","policy_version":"babel.integrity.v1","source_signals":[],"expected_revision":revision,"idempotency_key":key})
}
fn appeal(revision: u64, key: &str) -> Value {
    json!({"details":"Appellant private explanation sufficiently detailed","expected_revision":revision,"idempotency_key":key})
}

#[tokio::test]
async fn moderation_http_auth_redaction_review_appeal_retry_and_restart() {
    let f = Fixture::new();
    let app = f.app("");
    let author = register(&app, "author").await;
    let reporter = register(&app, "reporter").await;
    let reviewer = register(&app, "reviewer").await;
    let independent = register(&app, "independent").await;
    let stranger = register(&app, "stranger").await;
    let (_, published) = call(
        &app,
        "POST",
        "/objects/text",
        Some(&author),
        json!({"author_id":author.id,"text":"Moderation public history"}),
    )
    .await;
    let object = published["object"]["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            "/moderation/access",
            Some(&reviewer),
            Value::Null
        )
        .await
        .1["can_review"],
        false
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/moderation/reports?scope=queue",
            Some(&reviewer),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    drop(app);
    let ids = format!(
        "{},{},{},{}",
        reviewer.id, independent.id, author.id, reporter.id
    );
    let app = f.app(&ids);
    for (method, path, body) in [
        ("GET", "/moderation/access", Value::Null),
        ("POST", "/moderation/reports", report(object, "anon")),
        ("GET", "/moderation/reports", Value::Null),
    ] {
        assert_eq!(
            call(&app, method, path, None, body).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, initial) = call(
        &app,
        "POST",
        "/moderation/reports",
        Some(&reporter),
        report(object, "one"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{initial}");
    let path = format!("/moderation/reports/{}", initial["id"].as_str().unwrap());
    let decisions = format!("{path}/decisions");
    let appeals = format!("{path}/appeals");
    assert_eq!(
        call(&app, "GET", &path, Some(&stranger), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &decisions,
            Some(&stranger),
            decision("restrict", 1, "no-role")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for actor in [&author, &reporter] {
        assert_eq!(
            call(
                &app,
                "POST",
                &decisions,
                Some(actor),
                decision("restrict", 1, "self")
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    for field in [
        "reporter_id",
        "reviewer_id",
        "actor_id",
        "author_id",
        "identity_id",
    ] {
        let mut forged = report(object, "forged");
        forged[field] = json!(stranger.id);
        assert!(
            call(&app, "POST", "/moderation/reports", Some(&reporter), forged)
                .await
                .0
                .is_client_error()
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            &decisions,
            Some(&reviewer),
            decision("restrict", 2, "cas")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, restricted) = call(
        &app,
        "POST",
        &decisions,
        Some(&reviewer),
        decision("restrict", 1, "restrict"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restricted}");
    let (status, appealed) = call(&app, "POST", &appeals, Some(&author), appeal(2, "appeal")).await;
    assert_eq!(status, StatusCode::OK, "{appealed}");
    assert_eq!(
        call(
            &app,
            "POST",
            &decisions,
            Some(&reviewer),
            decision("no_action", 3, "same-reviewer")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &decisions,
            Some(&reviewer),
            decision("restrict", 1, "restrict")
        )
        .await
        .1,
        restricted
    );
    assert_eq!(
        call(&app, "POST", &appeals, Some(&author), appeal(2, "appeal"))
            .await
            .1,
        appealed
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &appeals,
            Some(&author),
            appeal(2, "another-appeal")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, closed) = call(
        &app,
        "POST",
        &decisions,
        Some(&independent),
        decision("no_action", 3, "reverse"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["status"], "closed");
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/objects/{object}"),
            None,
            Value::Null
        )
        .await
        .1["object"],
        published["object"]
    );
    drop(app);
    let app = f.app(&format!("{},{}", reviewer.id, independent.id));
    let author_view = call(&app, "GET", &path, Some(&author), Value::Null).await.1;
    assert!(
        author_view["reporter_id"].is_null()
            && author_view["details"].is_null()
            && author_view["reason"].is_null()
    );
    assert!(author_view["appeal"]["details"].is_string());
    let reporter_view = call(&app, "GET", &path, Some(&reporter), Value::Null)
        .await
        .1;
    assert!(
        reporter_view["appeal"]["details"].is_null()
            && reporter_view["appeal"]["appellant_id"].is_null()
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/moderation/reports",
            Some(&reporter),
            report(object, "one")
        )
        .await
        .1,
        initial
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/moderation/reports?scope=affected",
            Some(&author),
            Value::Null
        )
        .await
        .1["items"][0],
        author_view
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/moderation/reports?scope=queue",
            Some(&reviewer),
            Value::Null
        )
        .await
        .1["items"],
        json!([])
    );
    drop(app);
    let app = f.app("");
    assert_eq!(
        call(
            &app,
            "POST",
            &decisions,
            Some(&reviewer),
            decision("restrict", 1, "restrict")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "GET", &path, Some(&reviewer), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/moderation/access")
                .header("authorization", format!("Bearer {}", reporter.token))
                .header(
                    "x-babel-surface-document",
                    "00000000-0000-0000-0000-000000000000",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_client_error());
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/auth/session",
            Some(&reporter),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/moderation/access",
            Some(&reporter),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn moderation_transport_bounds_private_failures_and_default_deny_rpc() {
    let f = Fixture::new();
    let app = f.app("");
    let actor = register(&app, "actor").await;
    for query in [
        "scope=bad",
        "limit=101",
        "limit=0",
        "before=0",
        "before=9007199254740992",
        "scope=mine&actor_id=forged",
    ] {
        assert!(
            call(
                &app,
                "GET",
                &format!("/moderation/reports?{query}"),
                Some(&actor),
                Value::Null
            )
            .await
            .0
            .is_client_error()
        );
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/moderation/reports")
                .header("authorization", format!("Bearer {}", actor.token))
                .header("content-type", "application/json")
                .body(Body::from(" ".repeat(65537)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let (_, object) = call(
        &app,
        "POST",
        "/objects/text",
        Some(&actor),
        json!({"author_id":actor.id,"text":"Object for bounds"}),
    )
    .await;
    let object = object["object"]["id"].as_str().unwrap();
    for details in ["short".into(), "a".repeat(4001), " ".repeat(16001)] {
        let mut r = report(object, "bad");
        r["details"] = json!(details);
        assert!(
            call(&app, "POST", "/moderation/reports", Some(&actor), r)
                .await
                .0
                .is_client_error()
        );
    }
    let mut r = report(object, "unicode");
    r["details"] = json!("\u{1f680}".repeat(20));
    assert_eq!(
        call(&app, "POST", "/moderation/reports", Some(&actor), r)
            .await
            .0,
        StatusCode::OK
    );
    let catalog = babel_rpc::babel_rpc_catalog().unwrap();
    assert!(
        !catalog
            .methods
            .iter()
            .any(|m| m.method.as_str().contains("moderation"))
    );
    let mut rpc = serde_json::to_value(
        babel_rpc::RpcRequestEnvelope::new(
            &catalog,
            "forged",
            "babel.identity.current.v1",
            babel_rpc::RpcBinding::host("moderation", "https://babel.test").unwrap(),
            json!({}),
        )
        .unwrap(),
    )
    .unwrap();
    rpc["method"] = json!("babel.moderation.reports.create.v1");
    rpc["payload"] = report(object, "rpc");
    assert!(
        call(&app, "POST", "/rpc", Some(&actor), rpc)
            .await
            .0
            .is_client_error()
    );
}
