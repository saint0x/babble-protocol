use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babel_api::{ApiState, router};
use babel_identity::IdentityKind;
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tower::ServiceExt;

struct Fixture {
    root: PathBuf,
    author: String,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-profile-http-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let identity = node
            .create_identity(IdentityKind::Person, "real-profile-handle")
            .unwrap();
        for text in [
            "First public Object",
            "Second public Object",
            "Third public Object",
        ] {
            node.publish_text(&identity.id, text).unwrap();
        }
        Self {
            root,
            author: identity.id.to_string(),
        }
    }
    fn app(&self) -> Router {
        router(ApiState::new(
            LocalNode::open(&self.root, LocalProvider::default()).unwrap(),
        ))
    }
    fn uri(&self, query: &str) -> String {
        format!("/identities/{}/objects{query}", self.author)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn profiles_public_http_authoritative_identity_and_pagination() {
    let f = Fixture::new();
    let app = f.app();
    let (status, first) = get(&app, &f.uri("?limit=2")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["identity"]["id"], f.author);
    assert_eq!(first["identity"]["handle"], "real-profile-handle");
    assert_eq!(first["objects"].as_array().unwrap().len(), 2);
    assert!(first.get("total").is_none());
    let cursor = first["next_cursor"].as_str().unwrap().replace('|', "%7C");
    let (status, last) = get(&app, &f.uri(&format!("?limit=2&cursor={cursor}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(last["objects"].as_array().unwrap().len(), 1);
    assert!(last["next_cursor"].is_null());
    assert!(
        !first["objects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|object| object["id"] == last["objects"][0]["id"])
    );
}

#[tokio::test]
async fn profiles_http_validates_requests_and_has_no_write_endpoint() {
    let f = Fixture::new();
    let app = f.app();
    for query in [
        "?limit=0",
        "?limit=51",
        "?cursor=garbage",
        "?limit=no",
        "?unexpected=1",
    ] {
        assert!(
            get(&app, &f.uri(query)).await.0.is_client_error(),
            "{query}"
        );
    }
    assert_eq!(
        get(&app, &format!("/identities/id_{}/objects", "0".repeat(64)))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(f.uri(""))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(!response.status().is_success());
}

#[tokio::test]
async fn profiles_http_snapshot_conflict_is_refreshable() {
    let f = Fixture::new();
    let (_, first) = get(&f.app(), &f.uri("?limit=1")).await;
    let cursor = first["next_cursor"].as_str().unwrap().replace('|', "%7C");
    let mut node = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    node.publish_text(
        &babel_types::IdentityId::new_unchecked(&f.author),
        "New Object",
    )
    .unwrap();
    let app = router(ApiState::new(node));
    let (status, body) = get(&app, &f.uri(&format!("?cursor={cursor}"))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "conflict");
    let (status, page) = get(&app, &f.uri("")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["objects"].as_array().unwrap().len(), 4);
}
