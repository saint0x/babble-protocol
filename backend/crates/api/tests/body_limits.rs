use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babel_api::{ApiState, router};
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use babel_rpc::{RpcBinding, RpcRequestEnvelope, babel_rpc_catalog};
use babel_store::FileStore;
use babel_types::Hash;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tower::ServiceExt;

const MAX_BODY_BYTES: usize = 16 * 1024 * 1024 + 64 * 1024;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self {
            root: std::env::temp_dir().join(format!(
                "babel-body-limits-{}-{}-{}",
                std::process::id(),
                time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            )),
        }
    }

    fn app(&self) -> Router {
        router(ApiState::new(
            LocalNode::open(&self.root, LocalProvider::default()).unwrap(),
        ))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

async fn post(app: &Router, path: &str, token: Option<&str>, body: Vec<u8>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, body)
}

async fn register(app: &Router) -> String {
    let (status, body) = post(
        app,
        "/auth/register",
        None,
        serde_json::to_vec(&json!({
            "handle": "body-limit-author",
            "kind": "Person",
            "password": "correct horse bounded uploads",
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

fn upload_body(path: &str, payload: &[u8]) -> Vec<u8> {
    let input =
        json!({"media_type": "application/octet-stream", "bytes_hex": hex::encode(payload)});
    if path == "/rpc" {
        let request = RpcRequestEnvelope::new(
            &babel_rpc_catalog().unwrap(),
            "body-limit-upload",
            "babel.media.blob.put.v1",
            RpcBinding::host("body-limit-tests", "https://babel.test").unwrap(),
            input,
        )
        .unwrap()
        .with_idempotency_key(format!("upload-{}", Hash::from_bytes(payload)));
        serde_json::to_vec(&request).unwrap()
    } else {
        serde_json::to_vec(&input).unwrap()
    }
}

#[tokio::test]
async fn authenticated_rest_and_rpc_uploads_accept_exactly_eight_mib_of_file_bytes() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let token = register(&app).await;
    for (index, path) in ["/media/blobs", "/rpc"].into_iter().enumerate() {
        let payload = vec![index as u8; 8 * 1024 * 1024];
        let hash = Hash::from_bytes(&payload);
        let body = upload_body(path, &payload);
        assert!(body.len() > 2 * 1024 * 1024);
        assert!(body.len() < MAX_BODY_BYTES);

        let (status, body) = post(&app, path, Some(&token), body).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        let result = if path == "/rpc" {
            assert_eq!(body["error"], Value::Null, "{body}");
            &body["result"]
        } else {
            &body
        };
        assert_eq!(result["blob"]["integrity"], hash.as_str());
        assert_eq!(result["blob"]["size_bytes"], payload.len() as u64);
        assert_eq!(result["bytes_hex"], Value::Null);
        assert_eq!(
            FileStore::open(&fixture.root)
                .unwrap()
                .get_blob_bounded(&hash, payload.len())
                .unwrap(),
            Some(payload),
        );
    }
}

#[tokio::test]
async fn authenticated_rest_and_rpc_uploads_reject_beyond_envelope_headroom_before_storage() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let token = register(&app).await;
    for (index, path) in ["/media/blobs", "/rpc"].into_iter().enumerate() {
        let payload = vec![index as u8; MAX_BODY_BYTES / 2];
        let hash = Hash::from_bytes(&payload);
        // Hex data alone fills the limit; the valid JSON envelope exceeds it.
        let body = upload_body(path, &payload);
        assert!(body.len() > MAX_BODY_BYTES);

        let (status, body) = post(&app, path, Some(&token), body).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{path}: {body}");
        assert_eq!(body, Value::Null);
        assert!(
            !FileStore::open(&fixture.root)
                .unwrap()
                .contains_blob(&hash)
                .unwrap(),
            "{path}: rejected upload was stored",
        );
    }
}

#[tokio::test]
async fn envelope_headroom_does_not_allow_oversized_decoded_uploads() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let token = register(&app).await;
    for (index, path) in ["/media/blobs", "/rpc"].into_iter().enumerate() {
        let payload = vec![index as u8; 8 * 1024 * 1024 + 1];
        let hash = Hash::from_bytes(&payload);
        let body = upload_body(path, &payload);
        assert!(body.len() < MAX_BODY_BYTES);
        let (status, response) = post(&app, path, Some(&token), body).await;
        if path == "/rpc" {
            assert_eq!(status, StatusCode::OK);
            assert!(!response["error"].is_null(), "{response}");
            assert!(response["result"].is_null(), "{response}");
        } else {
            assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
        }
        assert!(
            !FileStore::open(&fixture.root)
                .unwrap()
                .contains_blob(&hash)
                .unwrap()
        );
    }
}
