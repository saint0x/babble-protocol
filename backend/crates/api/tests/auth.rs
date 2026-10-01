use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babble_api::{ApiState, router};
use babble_judgment_local::LocalProvider;
use babble_node::LocalNode;
use babble_rpc::{RpcBinding, RpcRequestEnvelope, babble_rpc_catalog};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tower::ServiceExt;

const PASSWORD: &str = "correct horse durable session";
const DOCUMENT: &str = "550e8400-e29b-41d4-a716-446655440000";

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-auth-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        Self { root }
    }
    fn app(&self) -> Router {
        app(&self.root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn app(root: &Path) -> Router {
    router(ApiState::new(
        LocalNode::open(root, LocalProvider::default()).unwrap(),
    ))
}

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    request_with_headers(app, method, uri, token, body, &[]).await
}

async fn request_with_headers(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Value,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
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
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, body)
}

#[derive(Clone)]
struct Account {
    id: String,
    token: String,
}
async fn register(app: &Router, handle: &str) -> Account {
    let (status, body) = request(
        app,
        "POST",
        "/auth/register",
        None,
        json!({"handle":handle,"kind":"Person","password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        time::OffsetDateTime::parse(
            body["expires_at"].as_str().unwrap(),
            &time::format_description::well_known::Rfc3339
        )
        .is_ok()
    );
    Account {
        id: body["identity"]["id"].as_str().unwrap().into(),
        token: body["token"].as_str().unwrap().into(),
    }
}

fn envelope(method: &str, binding: RpcBinding, payload: Value) -> Value {
    static OPERATION: AtomicU64 = AtomicU64::new(0);
    let catalog = babble_rpc_catalog().unwrap();
    serde_json::to_value(
        RpcRequestEnvelope::new(&catalog, "auth-test-request", method, binding, payload)
            .unwrap()
            .with_idempotency_key(format!(
                "auth-test-operation-{}",
                OPERATION.fetch_add(1, Ordering::Relaxed)
            )),
    )
    .unwrap()
}
fn host() -> RpcBinding {
    RpcBinding::host("web-host", "https://babble.test").unwrap()
}
async fn rpc(
    app: &Router,
    account: &Account,
    method: &str,
    binding: RpcBinding,
    payload: Value,
) -> (StatusCode, Value) {
    let headers = if binding.object_id.is_some() && binding.surface_session_id.is_some() {
        vec![("x-babble-surface-document", DOCUMENT)]
    } else {
        vec![]
    };
    request_with_headers(
        app,
        "POST",
        "/rpc",
        Some(&account.token),
        envelope(method, binding, payload),
        &headers,
    )
    .await
}

async fn register_document(app: &Router, account: &Account, session: &str) {
    let result = request(
        app,
        "PUT",
        &format!("/runtime/surfaces/sessions/{session}/document"),
        Some(&account.token),
        json!({"document_id":DOCUMENT}),
    )
    .await;
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    assert_eq!(
        result.1,
        json!({"session_id":session,"document_id":DOCUMENT})
    );
}
async fn publish(app: &Router, account: &Account, text: &str) -> String {
    let (status, body) = request(
        app,
        "POST",
        "/objects/text",
        Some(&account.token),
        json!({"author_id":account.id,"text":text}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["object"]["id"].as_str().unwrap().into()
}

#[tokio::test]
async fn auth_account_restart_logout_expiry_and_relogin_restore_signing() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "same-handle").await;
    let bob = register(&app, "same-handle").await;
    assert_ne!(alice.id, bob.id);
    assert_ne!(alice.token, bob.token);
    let object = publish(&app, &alice, "before restart").await;
    let rotated = request(
        &app,
        "POST",
        &format!("/identities/{}/keys/rotate", alice.id),
        Some(&alice.token),
        json!({"scope":"Root","reason":"durable key rotation"}),
    )
    .await;
    assert_eq!(rotated.0, StatusCode::OK, "{}", rotated.1);
    drop(app);
    let app = fixture.app();
    let (status, body) = request(
        &app,
        "GET",
        "/auth/session",
        Some(&alice.token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["identity"]["id"], alice.id);
    publish(&app, &alice, "after restart and rotation").await;
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/objects/{object}"),
            None,
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "DELETE",
            "/auth/session",
            Some(&alice.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    drop(app);
    let app = fixture.app();
    assert_eq!(
        request(
            &app,
            "GET",
            "/auth/session",
            Some(&alice.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "GET", "/auth/session", Some(&bob.token), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    let (status, body) = request(
        &app,
        "POST",
        "/auth/login",
        None,
        json!({"identity_id":alice.id,"password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let refreshed = Account {
        id: alice.id.clone(),
        token: body["token"].as_str().unwrap().into(),
    };
    publish(&app, &refreshed, "returning after logout").await;
    let db = rusqlite::Connection::open(fixture.root.join("auth/accounts.sqlite3")).unwrap();
    db.execute(
        "UPDATE sessions SET expires_at=0 WHERE identity_id=?1",
        [&alice.id],
    )
    .unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            "/auth/session",
            Some(&refreshed.token),
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
            "/auth/login",
            None,
            json!({"identity_id":alice.id,"password":PASSWORD})
        )
        .await
        .0,
        StatusCode::OK
    );
    let hashes: Vec<String> = db
        .prepare("SELECT password_hash FROM accounts")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(hashes.len(), 2);
    assert_ne!(hashes[0], hashes[1]);
    assert!(
        hashes
            .iter()
            .all(|hash| hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"))
    );
    let disk = fs::read(fixture.root.join("auth/accounts.sqlite3")).unwrap();
    for secret in [PASSWORD, &alice.token, &bob.token, &refreshed.token] {
        assert!(
            !disk
                .windows(secret.len())
                .any(|part| part == secret.as_bytes())
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(fixture.root.join("auth"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(fixture.root.join("auth/accounts.sqlite3"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        for key in fs::read_dir(fixture.root.join("signing_keys")).unwrap() {
            assert_eq!(
                key.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[tokio::test]
async fn auth_rejects_unowned_identity_credentials_and_direct_rest_impersonation() {
    let fixture = Fixture::new();
    let mut node = LocalNode::open(&fixture.root, LocalProvider::default()).unwrap();
    let seeded = node
        .create_identity(babble_identity::IdentityKind::Person, "seeded")
        .unwrap();
    let app = router(ApiState::new(node));
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let mut errors = Vec::new();
    for identity in [&alice.id, seeded.id.as_str(), "unknown"] {
        let response = request(
            &app,
            "POST",
            "/auth/login",
            None,
            json!({"identity_id":identity,"password":"wrong password value"}),
        )
        .await;
        assert_eq!(response.0, StatusCode::UNAUTHORIZED);
        errors.push(response.1);
    }
    assert!(errors.windows(2).all(|pair| pair[0] == pair[1]));
    for password in ["x".repeat(11), "x".repeat(1025)] {
        assert_eq!(
            request(
                &app,
                "POST",
                "/auth/register",
                None,
                json!({"handle":"bad","kind":"Person","password":password})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    for token in [None, Some("forged")] {
        assert_eq!(
            request(
                &app,
                "POST",
                "/objects/text",
                token,
                json!({"author_id":alice.id,"text":"forged"})
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
            "/objects/text",
            Some(&bob.token),
            json!({"author_id":alice.id,"text":"forged"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut binding = host();
    binding.identity_id = Some(alice.id.clone());
    assert_eq!(
        rpc(
            &app,
            &bob,
            "babble.object.publish_text.v1",
            binding,
            json!({"author_id":bob.id,"text":"forged binding"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/identities",
            Some(&alice.token),
            json!({"handle":"legacy","kind":"Person"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        rpc(
            &app,
            &alice,
            "babble.identity.create.v1",
            host(),
            json!({"handle":"legacy","kind":"Person"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for route in ["/observability", "/events", "/runtime/surfaces/health"] {
        assert_eq!(
            request(&app, "GET", route, Some(&alice.token), Value::Null)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    for method in [
        "babble.observability.snapshot.v1",
        "babble.events.import.v1",
        "babble.consensus.checkpoint.publish.v1",
    ] {
        assert_eq!(
            rpc(&app, &alice, method, host(), json!({})).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let mut unknown = envelope("babble.object.get.v1", host(), json!({}));
    unknown["method"] = json!("babble.future.unclassified.v1");
    assert_eq!(
        request(&app, "POST", "/rpc", Some(&alice.token), unknown)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/personalization/sync/envelopes?identity_id={}", alice.id),
            Some(&bob.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("/identities/{}/keys/rotate", alice.id),
            Some(&bob.token),
            json!({"scope":"Root","reason":"steal"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/media/blobs",
            Some(&alice.token),
            json!({"media_type":"text/plain","bytes_hex":"6869"})
        )
        .await
        .0,
        StatusCode::OK
    );
    publish(&app, &alice, "owner still works").await;
}

async fn controller(
    app: &Router,
    author: &Account,
    target: &str,
    surface: bool,
) -> (String, Vec<Value>) {
    let capabilities = vec![
        json!({"id":"babble.social.reply","version":1,"scope":{"object_id":target}}),
        json!({"id":"babble.social.share","version":1,"scope":{"object_id":target}}),
        json!({"id":"babble.social.follow","version":1,"scope":{"object_id":target}}),
        json!({"id":"babble.storage.local","version":1,"scope":{"namespace":"self"}}),
        json!({"id":"babble.storage.object","version":1,"scope":{"namespace":"self"}}),
    ];
    let mut draft = serde_json::to_value(
        babble_authoring::ObjectDraft::text("authenticated controller").unwrap(),
    )
    .unwrap();
    draft["capabilities"] = json!(capabilities);
    if surface {
        let (status, uploaded) = request(app,"POST","/media/blobs",Some(&author.token),json!({"media_type":"text/html","bytes_hex":hex::encode(b"<!doctype html><p>Auth test</p>")})).await;
        assert_eq!(status, StatusCode::OK);
        let hash = uploaded["blob"]["integrity"].as_str().unwrap();
        let uri = format!("babble://blobs/{hash}");
        draft["surfaces"] = json!([{"role":"Feed","target":"Web","entry":uri,"integrity":hash}]);
        draft["resources"] = json!([{"uri":uri,"integrity":hash,"media_type":"text/html"}]);
    }
    let (status, body) = request(
        app,
        "POST",
        "/objects",
        Some(&author.token),
        json!({"author_id":author.id,"draft":draft}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (body["object"]["id"].as_str().unwrap().into(), capabilities)
}

async fn grants(
    app: &Router,
    account: &Account,
    object: &str,
    capabilities: &[Value],
) -> Vec<String> {
    let mut ids = Vec::new();
    for capability in capabilities {
        let (status, body) = request(app,"POST","/capabilities/grants",Some(&account.token),json!({"author_id":account.id,"object_id":object,"capability":capability,"decision":"approved"})).await;
        if capability["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("babble.social."))
        {
            assert_eq!(
                status,
                StatusCode::CONFLICT,
                "social reusable consent must be rejected: {body}"
            );
            continue;
        }
        assert_eq!(status, StatusCode::OK, "{body}");
        ids.push(
            body["event"]["payload"]["grant"]["id"]
                .as_str()
                .unwrap()
                .into(),
        );
    }
    ids
}

fn bound(object: &str, session: &str, grants: Vec<String>) -> RpcBinding {
    RpcBinding::object(
        object,
        session,
        "surface-host",
        "https://babble.test",
        grants,
    )
    .unwrap()
}

fn host_object(object: &str, grants: Vec<String>) -> RpcBinding {
    let mut binding = host();
    binding.object_id = Some(object.into());
    binding.capability_grants = grants;
    binding
}

#[tokio::test]
async fn auth_storage_grants_cannot_be_stolen_and_revocation_survives_restart() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let target = publish(&app, &bob, "reply target").await;
    let (object, caps) = controller(&app, &alice, &target, false).await;
    let alice_grants = grants(&app, &alice, &object, &caps).await;
    let binding = host_object(&object, alice_grants.clone());
    for (method, payload) in [
        (
            "babble.storage.local.set.v1",
            json!({"key":"settings","value":{"a":1}}),
        ),
        (
            "babble.storage.object.set.v1",
            json!({"key":"settings","value":{"a":2}}),
        ),
    ] {
        let result = rpc(&app, &alice, method, binding.clone(), payload).await;
        assert_eq!(result.0, StatusCode::OK, "{}", result.1);
        assert!(result.1["error"].is_null(), "{}", result.1);
    }
    assert_eq!(
        rpc(
            &app,
            &bob,
            "babble.social.reply.v1",
            binding.clone(),
            json!({"author_id":bob.id,"target_object_id":target,"text":"stolen"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let bobs_grants = grants(&app, &bob, &object, &caps).await;
    assert_eq!(
        rpc(
            &app,
            &alice,
            "babble.storage.local.get.v1",
            host_object(&object, bobs_grants),
            json!({"key":"settings"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/capabilities/revocations",
            Some(&bob.token),
            json!({"author_id":bob.id,"object_id":object,"grant_id":alice_grants[1]})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/capabilities/revocations",
            Some(&alice.token),
            json!({"author_id":alice.id,"object_id":object,"grant_id":alice_grants[1]})
        )
        .await
        .0,
        StatusCode::OK
    );
    drop(app);
    let app = fixture.app();
    let read = rpc(
        &app,
        &alice,
        "babble.storage.local.get.v1",
        host_object(&object, vec![alice_grants[0].clone()]),
        json!({"key":"settings"}),
    )
    .await;
    assert_eq!(
        read.1["result"]["entry"]["value"],
        json!({"a":1}),
        "{}",
        read.1
    );
    let rejected = rpc(
        &app,
        &alice,
        "babble.storage.object.get.v1",
        binding,
        json!({"key":"settings"}),
    )
    .await;
    assert!(!rejected.1["error"].is_null(), "{}", rejected.1);
}

#[path = "auth/documents.rs"]
mod documents;
#[path = "auth/leases.rs"]
mod leases;
#[path = "auth/realtime.rs"]
mod realtime;
#[path = "auth/social_media.rs"]
mod social_media;
#[path = "auth/surfaces.rs"]
mod surfaces;

#[tokio::test]
async fn publication_retry_http_reauthenticates_after_restart_and_logout() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "retry-owner").await;
    let bob = register(&app, "retry-other").await;
    let operation = envelope(
        "babble.object.publish_text.v1",
        host(),
        json!({"author_id":alice.id,"text":"one durable post"}),
    );
    let (status, first) =
        request(&app, "POST", "/rpc", Some(&alice.token), operation.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(first["error"].is_null(), "{first}");
    drop(app);
    let app = fixture.app();
    let replay = request(&app, "POST", "/rpc", Some(&alice.token), operation.clone()).await;
    assert_eq!(replay.0, StatusCode::OK);
    assert_eq!(replay.1["result"], first["result"]);
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(bob.token.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            request(&app, "POST", "/rpc", token, operation.clone())
                .await
                .0,
            expected
        );
    }
    assert_eq!(
        request(
            &app,
            "DELETE",
            "/auth/session",
            Some(&alice.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "POST", "/rpc", Some(&alice.token), operation.clone())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, login) = request(
        &app,
        "POST",
        "/auth/login",
        None,
        json!({"identity_id":alice.id,"password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut reconnected = operation;
    reconnected["binding"]["runtime_id"] = json!("new-browser-runtime");
    let replay = request(&app, "POST", "/rpc", login["token"].as_str(), reconnected).await;
    assert_eq!(replay.0, StatusCode::OK);
    assert_eq!(replay.1["result"], first["result"]);
    assert_eq!(
        babble_store::FileStore::open(&fixture.root)
            .unwrap()
            .list_objects()
            .unwrap()
            .len(),
        1
    );
}
