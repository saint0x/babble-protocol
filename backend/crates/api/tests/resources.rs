use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babble_api::{ApiState, router};
use babble_judgment_local::LocalProvider;
use babble_node::LocalNode;
use babble_rpc::{RpcBinding, RpcRequestEnvelope, babble_rpc_catalog};
use babble_types::Hash;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tower::ServiceExt;

const LIMIT: usize = 8 * 1024 * 1024;

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-resource-api-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        let node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        Self { root, node }
    }
    fn app(&self) -> Router {
        router(ApiState::new(
            LocalNode::open(&self.root, LocalProvider::default()).unwrap(),
        ))
    }
    fn path(&self, hash: &Hash) -> PathBuf {
        self.root.join("blobs").join(hash.as_str())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

async fn get(app: &Router, path: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), LIMIT * 2 + 4096)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

async fn rpc_blob(app: &Router, hash: &Hash) -> Value {
    let request = RpcRequestEnvelope::new(
        &babble_rpc_catalog().unwrap(),
        "resource-read",
        "babble.media.blob.get.v1",
        RpcBinding::host("resource-tests", "http://babble.test").unwrap(),
        json!({"hash":hash, "media_type":"text/javascript"}),
    )
    .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/rpc")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(
        &to_bytes(response.into_body(), LIMIT * 2 + 4096)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn resource_bytes_and_hash_survive_bounded_http_and_rpc_delivery() {
    let fixture = Fixture::new();
    let payload = b"window.babbleResource = 'verified bytes';";
    let blob = fixture
        .node
        .put_media_blob("text/javascript", payload)
        .unwrap();
    let app = fixture.app();
    let (status, headers, bytes) = get(
        &app,
        &format!(
            "/runtime/surfaces/blobs/{}?media_type=Text/JavaScript",
            blob.integrity
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, payload);
    assert_eq!(headers["content-type"], "text/javascript");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert!(
        headers["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("worker-src 'none'")
    );
    let rpc = rpc_blob(&app, &blob.integrity).await;
    assert_eq!(rpc["error"], Value::Null, "{rpc}");
    assert_eq!(rpc["result"]["bytes_hex"], hex::encode(payload));
    let (status, _, bytes) = get(
        &app,
        &format!("/media/blobs/{}?media_type=text/javascript", blob.integrity),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["bytes_hex"],
        hex::encode(payload)
    );
}

#[tokio::test]
async fn resource_limit_applies_to_sparse_native_files_on_every_public_read_path() {
    let fixture = Fixture::new();
    let hash = Hash::from_bytes(b"oversized resource");
    fs::File::create(fixture.path(&hash))
        .unwrap()
        .set_len(1_u64 << 40)
        .unwrap();
    let app = fixture.app();
    for route in ["runtime/surfaces/blobs", "media/blobs"] {
        let (status, _, bytes) =
            get(&app, &format!("/{route}/{hash}?media_type=text/javascript")).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "payload_too_large");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains(&LIMIT.to_string())
        );
    }
    let rpc = rpc_blob(&app, &hash).await;
    assert_eq!(rpc["error"]["code"], "QUOTA_EXCEEDED", "{rpc}");
    assert_eq!(rpc["result"], Value::Null);
}

#[tokio::test]
async fn resource_exact_limit_is_accepted_and_next_byte_is_rejected() {
    let fixture = Fixture::new();
    let payload = vec![b'x'; LIMIT];
    let blob = fixture
        .node
        .put_media_blob("text/javascript", &payload)
        .unwrap();
    let app = fixture.app();
    let path = format!(
        "/runtime/surfaces/blobs/{}?media_type=text/javascript",
        blob.integrity
    );
    let (status, _, bytes) = get(&app, &path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, payload);
    let media_path = format!("/media/blobs/{}?media_type=text/javascript", blob.integrity);
    let (status, _, bytes) = get(&app, &media_path).await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["bytes_hex"], hex::encode(&payload));
    let rpc = rpc_blob(&app, &blob.integrity).await;
    assert_eq!(rpc["error"], Value::Null, "{rpc}");
    assert_eq!(rpc["result"]["bytes_hex"], hex::encode(&payload));
    fs::OpenOptions::new()
        .write(true)
        .open(fixture.path(&blob.integrity))
        .unwrap()
        .set_len((LIMIT + 1) as u64)
        .unwrap();
    assert_eq!(get(&app, &path).await.0, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        get(&app, &media_path).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let rpc = rpc_blob(&app, &blob.integrity).await;
    assert_eq!(rpc["error"]["code"], "QUOTA_EXCEEDED", "{rpc}");
    assert_eq!(rpc["result"], Value::Null);
}

#[tokio::test]
async fn noncanonical_hashes_fail_before_lookup_on_every_public_read_path() {
    let fixture = Fixture::new();
    let blob = fixture
        .node
        .put_media_blob("text/html", b"original")
        .unwrap();
    let app = fixture.app();
    for value in [
        "g".repeat(64),
        blob.integrity.as_str().to_uppercase(),
        "-".repeat(64),
    ] {
        for route in ["runtime/surfaces/blobs", "media/blobs"] {
            let (status, _, bytes) =
                get(&app, &format!("/{route}/{value}?media_type=text/html")).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()["code"],
                "bad_request"
            );
        }
        let rpc = rpc_blob(&app, &Hash::new_unchecked(value)).await;
        assert_eq!(rpc["error"]["code"], "INVALID_INPUT", "{rpc}");
        assert_eq!(rpc["result"], Value::Null);
    }
}

#[tokio::test]
async fn resource_validation_and_tamper_failures_never_serve_unverified_bytes() {
    let fixture = Fixture::new();
    let blob = fixture
        .node
        .put_media_blob("text/html", b"original")
        .unwrap();
    let app = fixture.app();
    let missing = Hash::from_bytes(b"missing");
    let path = format!("/runtime/surfaces/blobs/{missing}");
    assert_eq!(
        get(&app, &format!("{path}?media_type=image/png")).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&app, &format!("{path}?media_type=text/html")).await.0,
        StatusCode::NOT_FOUND
    );
    fs::write(fixture.path(&blob.integrity), b"tampered executable bytes").unwrap();
    for route in ["runtime/surfaces/blobs", "media/blobs"] {
        let (status, _, bytes) = get(
            &app,
            &format!("/{route}/{}?media_type=text/html", blob.integrity),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(!String::from_utf8_lossy(&bytes).contains("tampered executable bytes"));
    }
    let rpc = rpc_blob(&app, &blob.integrity).await;
    assert_eq!(rpc["error"]["code"], "CONFLICT", "{rpc}");
    assert_eq!(rpc["result"], Value::Null);
}
