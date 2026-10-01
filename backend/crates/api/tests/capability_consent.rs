use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babel_api::{ApiState, router};
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use babel_rpc::{RpcBinding, RpcRequestEnvelope, babel_rpc_catalog};
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
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "babel-capability-consent-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
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
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Account {
    id: String,
    token: String,
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    request_with_headers(app, method, path, token, body, &[]).await
}

async fn request_with_headers(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        builder = builder.header("content-type", "application/json");
        Body::from(serde_json::to_vec(&body).unwrap())
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

fn ok(response: (StatusCode, Value)) -> Value {
    assert_eq!(response.0, StatusCode::OK, "{}", response.1);
    response.1
}

fn rpc_result(response: (StatusCode, Value)) -> Value {
    let body = ok(response);
    assert!(body["error"].is_null(), "{body}");
    assert!(!body["result"].is_null(), "{body}");
    body["result"].clone()
}

async fn register(app: &Router, handle: &str) -> Account {
    let body = ok(request(
        app,
        "POST",
        "/auth/register",
        None,
        json!({
            "handle": handle, "kind": "Person", "password": "capability consent test password"
        }),
    )
    .await);
    Account {
        id: body["identity"]["id"].as_str().unwrap().into(),
        token: body["token"].as_str().unwrap().into(),
    }
}

fn capability() -> Value {
    json!({"id":"babel.storage.local","version":1,"scope":{"namespace":"self"}})
}

async fn publish(app: &Router, author: &Account) -> String {
    let blob = ok(request(
        app,
        "POST",
        "/media/blobs",
        Some(&author.token),
        json!({
            "media_type":"text/html", "bytes_hex":hex::encode(b"<!doctype html><p>Consent</p>")
        }),
    )
    .await);
    let hash = blob["blob"]["integrity"].as_str().unwrap();
    let uri = format!("babel://blobs/{hash}");
    let mut draft = serde_json::to_value(
        babel_authoring::ObjectDraft::text("Capability consent fixture").unwrap(),
    )
    .unwrap();
    draft["capabilities"] = json!([capability()]);
    draft["surfaces"] = json!([{"role":"Feed","target":"Web","entry":uri,"integrity":hash}]);
    draft["resources"] = json!([{"uri":uri,"integrity":hash,"media_type":"text/html"}]);
    let published = ok(request(
        app,
        "POST",
        "/objects",
        Some(&author.token),
        json!({
            "author_id":author.id, "draft":draft
        }),
    )
    .await);
    published["object"]["id"].as_str().unwrap().into()
}

fn grant_payload(account: &Account, object: &str, decision: &str) -> Value {
    json!({"author_id":account.id,"object_id":object,"capability":capability(),"decision":decision})
}

async fn grant(app: &Router, account: &Account, object: &str, decision: &str) -> Value {
    ok(request(
        app,
        "POST",
        "/capabilities/grants",
        Some(&account.token),
        grant_payload(account, object, decision),
    )
    .await)
}

fn grant_id(body: &Value) -> String {
    body["event"]["payload"]["grant"]["id"]
        .as_str()
        .unwrap()
        .into()
}

async fn revoke(app: &Router, account: &Account, object: &str, id: &str) -> (StatusCode, Value) {
    request(
        app,
        "POST",
        "/capabilities/revocations",
        Some(&account.token),
        json!({
            "author_id":account.id,"object_id":object,"grant_id":id
        }),
    )
    .await
}

async fn prepare(app: &Router, account: Option<&Account>, object: &str) -> Value {
    ok(request(
        app,
        "POST",
        "/runtime/surfaces/prepare",
        account.map(|a| a.token.as_str()),
        json!({"object_id":object,"role":"Feed"}),
    )
    .await)["plan"]
        .clone()
}

fn host() -> RpcBinding {
    RpcBinding::host("consent-host", "https://babel.test").unwrap()
}

fn envelope(method: &str, binding: RpcBinding, payload: Value) -> Value {
    static OPERATION: AtomicU64 = AtomicU64::new(0);
    serde_json::to_value(
        RpcRequestEnvelope::new(
            &babel_rpc_catalog().unwrap(),
            "consent-request",
            method,
            binding,
            payload,
        )
        .unwrap()
        .with_idempotency_key(format!(
            "consent-operation-{}",
            OPERATION.fetch_add(1, Ordering::Relaxed)
        )),
    )
    .unwrap()
}

async fn rpc(app: &Router, account: &Account, operation: Value) -> (StatusCode, Value) {
    let headers = if operation["binding"]["object_id"].is_string()
        && operation["binding"]["surface_session_id"].is_string()
    {
        vec![(
            "x-babel-surface-document",
            "550e8400-e29b-41d4-a716-446655440000",
        )]
    } else {
        vec![]
    };
    request_with_headers(
        app,
        "POST",
        "/rpc",
        Some(&account.token),
        operation,
        &headers,
    )
    .await
}

async fn register_document(app: &Router, account: &Account, session: &str) {
    ok(request(
        app,
        "PUT",
        &format!("/runtime/surfaces/sessions/{session}/document"),
        Some(&account.token),
        json!({"document_id":"550e8400-e29b-41d4-a716-446655440000"}),
    )
    .await);
}

#[tokio::test]
async fn inspection_and_foreign_object_consent_are_viewer_owned_with_public_manifest() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let author = register(&app, "author").await;
    let viewer = register(&app, "viewer").await;
    let object = publish(&app, &author).await;
    let authors_grant = grant_id(&grant(&app, &author, &object, "approved").await);

    let inspection_path = format!("/objects/{object}/capabilities");
    let public = ok(request(&app, "GET", &inspection_path, None, Value::Null).await);
    let inspected = ok(request(
        &app,
        "GET",
        &inspection_path,
        Some(&viewer.token),
        Value::Null,
    )
    .await);
    assert_eq!(public, inspected);
    assert_eq!(inspected["decisions"][0]["status"], "requires_user");
    assert_eq!(inspected["grants"], json!([]));
    assert!(inspected["decisions"][0]["grant"].is_null());
    let author_inspection = ok(request(
        &app,
        "GET",
        &inspection_path,
        Some(&author.token),
        Value::Null,
    )
    .await);
    assert_eq!(author_inspection["manifest"], public["manifest"]);
    assert_eq!(author_inspection["decisions"][0]["status"], "granted");
    assert_eq!(author_inspection["grants"][0]["id"], authors_grant);
    assert!(!public["manifest"].is_null());
    let anonymous_rpc = rpc_result(
        request(
            &app,
            "POST",
            "/rpc",
            None,
            envelope(
                "babel.capabilities.inspect.v1",
                host(),
                json!({"object_id":object}),
            ),
        )
        .await,
    );
    assert_eq!(anonymous_rpc, public);
    let rpc_inspection = rpc_result(
        rpc(
            &app,
            &viewer,
            envelope(
                "babel.capabilities.inspect.v1",
                host(),
                json!({"object_id":object}),
            ),
        )
        .await,
    );
    assert_eq!(rpc_inspection, inspected);
    for account in [None, Some(&viewer)] {
        let plan = prepare(&app, account, &object).await;
        assert_eq!(plan["admission"], "needs_permission");
        assert!(plan["capability_decisions"][0]["grant"].is_null());
    }

    for token in [None, Some("forged-token")] {
        assert_eq!(
            request(
                &app,
                "POST",
                "/capabilities/grants",
                token,
                grant_payload(&viewer, &object, "approved")
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        request(
            &app,
            "POST",
            "/capabilities/grants",
            Some(&viewer.token),
            grant_payload(&author, &object, "approved")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut altered_scope = grant_payload(&viewer, &object, "approved");
    altered_scope["capability"]["scope"] = json!({"namespace":"other"});
    assert_eq!(
        request(
            &app,
            "POST",
            "/capabilities/grants",
            Some(&viewer.token),
            altered_scope
        )
        .await
        .0,
        StatusCode::CONFLICT
    );

    let consent = grant(&app, &viewer, &object, "approved").await;
    let viewers_grant = grant_id(&consent);
    assert_eq!(consent["event"]["actor"], viewer.id);
    assert_eq!(consent["grants"].as_array().unwrap().len(), 1);
    assert_eq!(consent["grants"][0]["id"], viewers_grant);
    assert_eq!(
        revoke(&app, &author, &object, &viewers_grant).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        revoke(&app, &viewer, &object, &authors_grant).await.0,
        StatusCode::FORBIDDEN
    );
    let plan = prepare(&app, Some(&viewer), &object).await;
    assert_eq!(plan["admission"], "ready");
    assert_eq!(
        plan["capability_decisions"][0]["grant"]["id"],
        viewers_grant
    );
    let rpc_plan = rpc_result(
        rpc(
            &app,
            &viewer,
            envelope(
                "babel.runtime.surface.prepare.v1",
                host(),
                json!({"object_id":object,"role":"Feed"}),
            ),
        )
        .await,
    );
    assert_eq!(rpc_plan["plan"], plan);

    let viewer_inspection = rpc_result(
        rpc(
            &app,
            &viewer,
            envelope(
                "babel.capabilities.inspect.v1",
                host(),
                json!({"object_id":object}),
            ),
        )
        .await,
    );
    assert_eq!(viewer_inspection["grants"], consent["grants"]);
    assert_eq!(viewer_inspection["decisions"], plan["capability_decisions"]);
    assert_eq!(viewer_inspection["manifest"], public["manifest"]);
    assert_eq!(
        ok(request(
            &app,
            "GET",
            &inspection_path,
            Some(&viewer.token),
            Value::Null
        )
        .await),
        viewer_inspection
    );
    assert_eq!(
        ok(request(&app, "GET", &inspection_path, None, Value::Null).await),
        public
    );

    ok(revoke(&app, &viewer, &object, &viewers_grant).await);
    assert_eq!(
        prepare(&app, Some(&viewer), &object).await["admission"],
        "needs_permission"
    );
    assert_eq!(
        prepare(&app, Some(&author), &object).await["admission"],
        "ready"
    );
}

#[tokio::test]
async fn denied_records_do_not_override_approvals_and_revoking_selected_duplicate_falls_back() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app, "consenter").await;
    let object = publish(&app, &account).await;
    let denied = grant_id(&grant(&app, &account, &object, "denied").await);
    let plan = prepare(&app, Some(&account), &object).await;
    assert_eq!(plan["admission"], "needs_permission");
    assert_eq!(plan["capability_decisions"][0]["status"], "requires_user");
    assert!(plan["capability_decisions"][0]["grant"].is_null());

    let first = grant_id(&grant(&app, &account, &object, "approved").await);
    let second = grant_id(&grant(&app, &account, &object, "approved").await);
    assert_ne!(first, second);
    grant(&app, &account, &object, "denied").await;
    let plan = prepare(&app, Some(&account), &object).await;
    assert_eq!(
        plan["admission"], "ready",
        "a later denial does not revoke an approval"
    );
    // Projection is ordered by grant ID; selection is not newest-decision-wins.
    let (selected, remaining) = if first > second {
        (&first, &second)
    } else {
        (&second, &first)
    };
    assert_eq!(
        plan["capability_decisions"][0]["grant"]["id"],
        selected.as_str()
    );
    ok(revoke(&app, &account, &object, selected).await);
    let plan = prepare(&app, Some(&account), &object).await;
    assert_eq!(plan["admission"], "ready");
    assert_eq!(
        plan["capability_decisions"][0]["grant"]["id"],
        remaining.as_str()
    );
    assert_eq!(
        revoke(&app, &account, &object, selected).await.0,
        StatusCode::NOT_FOUND
    );
    ok(revoke(&app, &account, &object, remaining).await);
    let plan = prepare(&app, Some(&account), &object).await;
    assert_eq!(plan["admission"], "needs_permission");
    assert_eq!(plan["capability_decisions"][0]["status"], "requires_user");
    assert!(plan["capability_decisions"][0]["grant"].is_null());

    drop(app);
    let app = fixture.app();
    assert_eq!(
        prepare(&app, Some(&account), &object).await["admission"],
        "needs_permission"
    );
    let inspection = ok(request(
        &app,
        "GET",
        &format!("/objects/{object}/capabilities"),
        Some(&account.token),
        Value::Null,
    )
    .await);
    let records = inspection["grants"].as_array().unwrap();
    assert_eq!(records.len(), 4);
    assert_eq!(
        records
            .iter()
            .filter(|g| !g["revoked_at"].is_null())
            .count(),
        2
    );
    assert!(
        records
            .iter()
            .any(|g| g["id"] == denied && g["decision"] == "denied")
    );
}

#[tokio::test]
async fn rpc_consent_retries_replay_the_event_but_reconcile_current_access_across_restart() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app, "retry-consenter").await;
    let object = publish(&app, &account).await;
    let operation = envelope(
        "babel.capabilities.grant.v1",
        host(),
        grant_payload(&account, &object, "approved"),
    );
    let mut missing_key = operation.clone();
    missing_key
        .as_object_mut()
        .unwrap()
        .remove("idempotency_key");
    let rejected = ok(rpc(&app, &account, missing_key).await);
    assert_eq!(rejected["error"]["code"], "INVALID_INPUT");

    let first = rpc_result(rpc(&app, &account, operation.clone()).await);
    let second = rpc_result(rpc(&app, &account, operation.clone()).await);
    assert_eq!(first, second);
    assert_eq!(second["grants"].as_array().unwrap().len(), 1);
    let mut changed = operation.clone();
    changed["payload"]["decision"] = json!("denied");
    assert_eq!(
        ok(rpc(&app, &account, changed).await)["error"]["code"],
        "CONFLICT"
    );
    for key in ["".to_owned(), " ".to_owned(), "x".repeat(257)] {
        let mut invalid = operation.clone();
        invalid["idempotency_key"] = json!(key);
        assert_eq!(
            ok(rpc(&app, &account, invalid).await)["error"]["code"],
            "INVALID_INPUT"
        );
    }
    let mut bound = operation.clone();
    bound["binding"]["object_id"] = json!(object);
    assert_eq!(rpc(&app, &account, bound).await.0, StatusCode::FORBIDDEN);

    let revocation = envelope(
        "babel.capabilities.revoke.v1",
        host(),
        json!({
            "author_id":account.id,"object_id":object,"grant_id":grant_id(&first)
        }),
    );
    let revoked = rpc_result(rpc(&app, &account, revocation.clone()).await);
    assert_eq!(
        rpc_result(rpc(&app, &account, revocation.clone()).await),
        revoked
    );
    let retried_grant = rpc_result(rpc(&app, &account, operation.clone()).await);
    assert_eq!(retried_grant["event"], first["event"]);
    assert!(!retried_grant["grants"][0]["revoked_at"].is_null());
    drop(app);
    let app = fixture.app();
    assert_eq!(
        rpc_result(rpc(&app, &account, revocation.clone()).await),
        revoked
    );
    assert_eq!(
        rpc_result(rpc(&app, &account, operation.clone()).await),
        retried_grant
    );
    assert_eq!(
        request(&app, "POST", "/rpc", None, operation).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        prepare(&app, Some(&account), &object).await["admission"],
        "needs_permission"
    );
    let fresh = grant(&app, &account, &object, "approved").await;
    let replayed = rpc_result(rpc(&app, &account, revocation).await);
    assert_eq!(replayed["event"], revoked["event"]);
    assert_eq!(replayed["grants"], fresh["grants"]);
    assert_eq!(replayed["grants"].as_array().unwrap().len(), 2);
    assert_eq!(
        prepare(&app, Some(&account), &object).await["admission"],
        "ready"
    );
}

#[tokio::test]
async fn concurrent_consent_retries_and_cross_actor_keys_do_not_duplicate_or_share_events() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "consent-alice").await;
    let bob = register(&app, "consent-bob").await;
    let object = publish(&app, &alice).await;
    let operation = envelope(
        "babel.capabilities.grant.v1",
        host(),
        grant_payload(&alice, &object, "approved"),
    );
    let (left, right) = tokio::join!(
        rpc(&app, &alice, operation.clone()),
        rpc(&app, &alice, operation.clone())
    );
    let first = rpc_result(left);
    assert_eq!(rpc_result(right), first);
    assert_eq!(first["grants"].as_array().unwrap().len(), 1);
    assert_eq!(
        rpc(&app, &bob, operation.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    let mut own = operation.clone();
    own["payload"]["author_id"] = json!(bob.id);
    let other = rpc_result(rpc(&app, &bob, own).await);
    assert_ne!(grant_id(&other), grant_id(&first));
    assert_eq!(other["grants"].as_array().unwrap().len(), 1);
    let mut collision = envelope(
        "babel.capabilities.revoke.v1",
        host(),
        json!({"author_id":alice.id,"object_id":object,"grant_id":grant_id(&first)}),
    );
    collision["idempotency_key"] = operation["idempotency_key"].clone();
    assert_eq!(
        ok(rpc(&app, &alice, collision).await)["error"]["code"],
        "CONFLICT"
    );
    assert_eq!(rpc_result(rpc(&app, &alice, operation).await), first);
}

#[tokio::test]
async fn rest_consent_keys_are_validated_and_survive_restart_without_regranting() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app, "rest-retry").await;
    let object = publish(&app, &account).await;
    let payload = grant_payload(&account, &object, "approved");
    let headers = [("idempotency-key", "rest-grant")];
    let first = ok(request_with_headers(
        &app,
        "POST",
        "/capabilities/grants",
        Some(&account.token),
        payload.clone(),
        &headers,
    )
    .await);
    assert_eq!(
        ok(request_with_headers(
            &app,
            "POST",
            "/capabilities/grants",
            Some(&account.token),
            payload.clone(),
            &headers
        )
        .await),
        first
    );
    for invalid in [
        vec![("idempotency-key", "")],
        vec![("idempotency-key", " ")],
        vec![("idempotency-key", "a"), ("idempotency-key", "b")],
    ] {
        assert_eq!(
            request_with_headers(
                &app,
                "POST",
                "/capabilities/grants",
                Some(&account.token),
                payload.clone(),
                &invalid
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let oversized = "x".repeat(257);
    assert_eq!(
        request_with_headers(
            &app,
            "POST",
            "/capabilities/grants",
            Some(&account.token),
            payload.clone(),
            &[("idempotency-key", &oversized)]
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut changed = payload.clone();
    changed["decision"] = json!("denied");
    assert_eq!(
        request_with_headers(
            &app,
            "POST",
            "/capabilities/grants",
            Some(&account.token),
            changed,
            &headers
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let revocation = json!({"author_id":account.id,"object_id":object,"grant_id":grant_id(&first)});
    assert_eq!(
        request_with_headers(
            &app,
            "POST",
            "/capabilities/revocations",
            Some(&account.token),
            revocation.clone(),
            &headers
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let revoked = ok(request_with_headers(
        &app,
        "POST",
        "/capabilities/revocations",
        Some(&account.token),
        revocation.clone(),
        &[("idempotency-key", "rest-revoke")],
    )
    .await);
    drop(app);
    let app = fixture.app();
    let retried = ok(request_with_headers(
        &app,
        "POST",
        "/capabilities/grants",
        Some(&account.token),
        payload,
        &headers,
    )
    .await);
    assert_eq!(retried["event"], first["event"]);
    assert_eq!(retried["grants"], revoked["grants"]);
    assert_eq!(
        ok(request_with_headers(
            &app,
            "POST",
            "/capabilities/revocations",
            Some(&account.token),
            revocation,
            &[("idempotency-key", "rest-revoke")]
        )
        .await),
        revoked
    );
}

#[tokio::test]
async fn revocation_retires_running_session_and_fresh_consent_requires_a_new_session() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let author = register(&app, "surface-author").await;
    let viewer = register(&app, "surface-viewer").await;
    let object = publish(&app, &author).await;
    let original = grant_id(&grant(&app, &viewer, &object, "approved").await);
    let started = ok(request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&viewer.token),
        json!({"object_id":object,"role":"Feed"}),
    )
    .await);
    let session = started["session"]["id"].as_str().unwrap();
    let path = format!("/runtime/surfaces/sessions/{session}");
    register_document(&app, &viewer, session).await;
    ok(request(
        &app,
        "POST",
        &format!("{path}/lifecycle"),
        Some(&viewer.token),
        json!({"lifecycle":"active","reason":"consent regression"}),
    )
    .await);
    let read = |id: &str| {
        envelope(
            "babel.storage.local.get.v1",
            RpcBinding::object(
                &object,
                session,
                "consent-host",
                "https://babel.test",
                vec![id.into()],
            )
            .unwrap(),
            json!({"key":"consent-check"}),
        )
    };
    let rejected_consent = grant_id(&grant(&app, &viewer, &object, "denied").await);
    let denied = ok(rpc(&app, &viewer, read(&rejected_consent)).await);
    assert_eq!(denied["error"]["code"], "CAPABILITY_DENIED");
    rpc_result(rpc(&app, &viewer, read(&original)).await);
    ok(revoke(&app, &viewer, &object, &original).await);
    assert_eq!(
        rpc(&app, &viewer, read(&original)).await.0,
        StatusCode::FORBIDDEN
    );
    let current = ok(request(&app, "GET", &path, Some(&viewer.token), Value::Null).await);
    assert_eq!(current["session"]["lifecycle"], "evicted");
    assert_eq!(current["session"]["plan"], started["session"]["plan"]);
    assert_eq!(
        current["session"]["events"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["reason"],
        "Surface permission revoked, expired or unavailable"
    );
    for (suffix, payload) in [
        ("heartbeat", json!({})),
        (
            "lifecycle",
            json!({"lifecycle":"active","reason":"cannot resurrect"}),
        ),
    ] {
        assert_eq!(
            request(
                &app,
                "POST",
                &format!("{path}/{suffix}"),
                Some(&viewer.token),
                payload
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        prepare(&app, Some(&viewer), &object).await["admission"],
        "needs_permission"
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            Some(&viewer.token),
            json!({"object_id":object,"role":"Feed"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let retried = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&viewer.token),
        json!({"object_id":object,"role":"Feed","session_id":session}),
    )
    .await;
    assert_eq!(retried.0, StatusCode::FORBIDDEN);

    let replacement = grant_id(&grant(&app, &viewer, &object, "approved").await);
    assert_eq!(
        rpc(&app, &viewer, read(&replacement)).await.0,
        StatusCode::FORBIDDEN
    );
    let fresh = ok(request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&viewer.token),
        json!({"object_id":object,"role":"Feed"}),
    )
    .await);
    let fresh_id = fresh["session"]["id"].as_str().unwrap();
    assert_ne!(fresh_id, session);
    register_document(&app, &viewer, fresh_id).await;
    rpc_result(
        rpc(
            &app,
            &viewer,
            envelope(
                "babel.storage.local.get.v1",
                RpcBinding::object(
                    &object,
                    fresh_id,
                    "consent-host",
                    "https://babel.test",
                    vec![replacement],
                )
                .unwrap(),
                json!({"key":"consent-check"}),
            ),
        )
        .await,
    );
}

#[tokio::test]
async fn receipt_selection_uses_only_the_viewers_authorized_bound_grant() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let author = register(&app, "receipt-author").await;
    let viewer = register(&app, "receipt-viewer").await;
    let object = publish(&app, &author).await;
    let author_id = grant_id(&grant(&app, &author, &object, "approved").await);
    let viewer_id = grant_id(&grant(&app, &viewer, &object, "approved").await);
    assert_ne!(author_id, viewer_id);
    let (caller, supplied, unbound) = if author_id < viewer_id {
        (&author, &author_id, &viewer_id)
    } else {
        (&viewer, &viewer_id, &author_id)
    };
    let started = ok(request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&caller.token),
        json!({"object_id":object,"role":"Feed"}),
    )
    .await);
    assert_eq!(
        started["session"]["plan"]["capability_decisions"][0]["grant"]["id"],
        supplied.as_str()
    );
    let session = started["session"]["id"].as_str().unwrap();
    let operation = |id: &str| {
        envelope(
            "babel.storage.local.get.v1",
            RpcBinding::object(
                &object,
                session,
                "consent-host",
                "https://babel.test",
                vec![id.into()],
            )
            .unwrap(),
            json!({"key":"receipt-selection"}),
        )
    };
    register_document(&app, caller, session).await;
    assert_eq!(
        rpc(&app, caller, operation(unbound)).await.0,
        StatusCode::FORBIDDEN
    );
    let result = rpc_result(rpc(&app, caller, operation(supplied)).await);
    assert_eq!(result["receipt"]["grant_id"], supplied.as_str());

    let unknown = babel_types::CapabilityGrantId::from_hash(&babel_types::Hash::from_bytes(
        b"unknown consent",
    ))
    .to_string();
    for ids in [
        vec![unknown.clone()],
        vec![supplied.clone(), unknown],
        vec![supplied.clone(), unbound.clone()],
        vec![supplied.clone(), supplied.clone()],
    ] {
        let mut call = operation(supplied);
        call["binding"]["capability_grants"] = json!(ids);
        assert_eq!(rpc(&app, caller, call).await.0, StatusCode::FORBIDDEN);
    }
    let denied = grant_id(&grant(&app, caller, &object, "denied").await);
    let rejected = ok(rpc(&app, caller, operation(&denied)).await);
    assert_eq!(rejected["error"]["code"], "CAPABILITY_DENIED");
    let mut mixed = operation(supplied);
    mixed["binding"]["capability_grants"] = json!([denied, supplied]);
    let result = rpc_result(rpc(&app, caller, mixed).await);
    assert_eq!(result["receipt"]["grant_id"], supplied.as_str());
    ok(revoke(&app, caller, &object, supplied).await);
    assert_eq!(
        rpc(&app, caller, operation(supplied)).await.0,
        StatusCode::FORBIDDEN,
        "another viewer's active grant cannot rescue a retired session"
    );
}

#[tokio::test]
async fn inspect_does_not_accept_forged_viewers_and_keeps_denied_revoked_records_private() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let author = register(&app, "inspect-author").await;
    let viewer = register(&app, "inspect-viewer").await;
    let object = publish(&app, &author).await;
    grant(&app, &author, &object, "approved").await;
    let denied = grant_id(&grant(&app, &viewer, &object, "denied").await);
    let issued = rpc_result(
        rpc(
            &app,
            &viewer,
            envelope(
                "babel.capabilities.grant.v1",
                host(),
                grant_payload(&viewer, &object, "approved"),
            ),
        )
        .await,
    );
    assert_eq!(issued["grants"].as_array().unwrap().len(), 2);
    let approved = grant_id(&issued);
    let revoked = rpc_result(
        rpc(
            &app,
            &viewer,
            envelope(
                "babel.capabilities.revoke.v1",
                host(),
                json!({"author_id":viewer.id,"object_id":object,"grant_id":approved}),
            ),
        )
        .await,
    );
    assert_eq!(revoked["grants"].as_array().unwrap().len(), 2);
    let path = format!("/objects/{object}/capabilities");
    let operation = envelope(
        "babel.capabilities.inspect.v1",
        host(),
        json!({"object_id":object}),
    );
    let own = rpc_result(rpc(&app, &viewer, operation.clone()).await);
    assert_eq!(own["grants"], revoked["grants"]);
    assert_eq!(own["decisions"][0]["status"], "requires_user");
    assert!(own["decisions"][0]["grant"].is_null());
    assert!(
        own["grants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == denied && g["decision"] == "denied")
    );
    assert!(
        own["grants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == approved && !g["revoked_at"].is_null())
    );
    assert_eq!(
        ok(request(&app, "GET", &path, Some(&viewer.token), Value::Null).await),
        own
    );
    let public = rpc_result(request(&app, "POST", "/rpc", None, operation.clone()).await);
    assert_eq!(public["grants"], json!([]));
    assert_eq!(public["manifest"], own["manifest"]);
    assert_eq!(
        ok(request(&app, "GET", &path, None, Value::Null).await),
        public
    );
    let other = rpc_result(rpc(&app, &author, operation.clone()).await);
    assert_eq!(other["grants"].as_array().unwrap().len(), 1);
    assert_eq!(other["decisions"][0]["status"], "granted");
    let mut forged = operation.clone();
    forged["binding"]["identity_id"] = json!(author.id);
    assert_eq!(
        rpc(&app, &viewer, forged.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "POST", "/rpc", None, forged).await.0,
        StatusCode::UNAUTHORIZED
    );
    for route in [&path, &format!("{path}?identity_id={}", author.id)] {
        assert_eq!(
            ok(request(&app, "GET", route, Some(&viewer.token), Value::Null).await),
            own
        );
        assert_eq!(
            request(&app, "GET", route, Some("forged-token"), Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let unknown =
        babel_types::ObjectId::from_hash(&babel_types::Hash::from_bytes(b"unknown object"));
    for token in [None, Some(viewer.token.as_str())] {
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("/objects/{unknown}/capabilities"),
                token,
                Value::Null
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        let missing = ok(request(
            &app,
            "POST",
            "/rpc",
            token,
            envelope(
                "babel.capabilities.inspect.v1",
                host(),
                json!({"object_id":unknown}),
            ),
        )
        .await);
        assert_eq!(missing["error"]["code"], "NOT_FOUND");
    }
}

#[test]
fn node_receipts_reject_inactive_bindings_and_never_substitute_unbound_grants() {
    use babel_authoring::{CapabilityGrantDraft, ObjectDraft};
    use babel_capabilities::{CapabilityGrant, GrantDecision};
    use babel_identity::IdentityKind;
    use babel_object::CapabilityRequest;
    use babel_types::{CapabilityGrantId, Hash, Timestamp};

    let fixture = Fixture::new();
    let mut node = LocalNode::open(&fixture.0, LocalProvider::default()).unwrap();
    let author = node
        .create_identity(IdentityKind::Person, "native-author")
        .unwrap();
    let viewer = node
        .create_identity(IdentityKind::Person, "native-viewer")
        .unwrap();
    let request: CapabilityRequest = serde_json::from_value(json!({
        "id":"babel.notifications.request", "version":1,
        "scope":{"categories":["game.turn"],"purpose":"Turn alerts"}
    }))
    .unwrap();
    let object = node
        .publish_draft(
            &author.id,
            ObjectDraft::text("Receipt isolation")
                .unwrap()
                .with_capability(request.clone())
                .unwrap(),
        )
        .unwrap();
    let mut issue = |actor: &babel_types::IdentityId, decision, expires_at| -> CapabilityGrant {
        let event = node
            .grant_capability_draft(
                actor,
                CapabilityGrantDraft {
                    object_id: object.id.clone(),
                    request: request.clone(),
                    decision,
                    expires_at,
                },
            )
            .unwrap();
        serde_json::from_value(event.payload["grant"].clone()).unwrap()
    };
    let first = issue(&author.id, GrantDecision::Approved, None);
    let second = issue(&viewer.id, GrantDecision::Approved, None);
    let denied = issue(&viewer.id, GrantDecision::Denied, None);
    let expired = issue(
        &viewer.id,
        GrantDecision::Approved,
        Some(Timestamp(time::OffsetDateTime::UNIX_EPOCH)),
    );
    let (selected, owner) = if first.id < second.id {
        (&first, &author.id)
    } else {
        (&second, &viewer.id)
    };
    let unknown = CapabilityGrantId::from_hash(&Hash::from_bytes(b"missing native grant"));
    let check = |node: &LocalNode<LocalProvider>, ids: &[String]| {
        [
            node.authorize_capability_binding(&object.id, "babel.notifications.request", 1, ids),
            node.notifications_request(&object.id, "Turn alerts", &["game.turn".into()], ids),
        ]
    };
    for ids in [
        vec![],
        vec![unknown.to_string()],
        vec![denied.id.to_string()],
        vec![expired.id.to_string()],
    ] {
        for result in check(&node, &ids) {
            assert!(
                result.is_err(),
                "inactive binding used an unbound approval: {result:?}"
            );
        }
    }
    for ids in [
        vec![selected.id.to_string()],
        vec![
            expired.id.to_string(),
            denied.id.to_string(),
            selected.id.to_string(),
        ],
    ] {
        for result in check(&node, &ids) {
            assert_eq!(result.unwrap().grant_id, selected.id);
        }
    }
    node.revoke_capability(owner, &object.id, &selected.id)
        .unwrap();
    for result in check(&node, &[selected.id.to_string()]) {
        assert!(
            result.is_err(),
            "revoked binding used another actor's approval: {result:?}"
        );
    }
}
