use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use babel_api::{
    ApiState, DiscoveryResponse, PrepareSurfaceResponse,
    config::{SeedProfile, ServerConfig},
    seed::apply_seed_profile,
    serve::configured_router,
};
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use babel_object::SurfaceRole;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

#[tokio::test]
async fn configured_server_serves_seeded_feed_and_surface_resources() {
    let root = unique_root("seeded-server");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let report =
        apply_seed_profile(&mut node, SeedProfile::CardFeed, "http://127.0.0.1:8787").unwrap();
    assert_eq!(report.inserted_objects, 5);

    let app = configured_router(
        ServerConfig {
            bind_addr: "127.0.0.1:8787".parse().unwrap(),
            public_origin: "http://127.0.0.1:8787".to_string(),
            store_root: root.clone(),
            seed_profile: Some(SeedProfile::CardFeed),
            cors_origins: vec!["http://127.0.0.1:4329".parse().unwrap()],
            judgment: babel_api::provider::JudgmentConfig::RustLocal,
            bundle_gateway: None,
        },
        ApiState::new(node),
    );

    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/social/following/id_test")
                .header(header::ORIGIN, "http://127.0.0.1:4329")
                .header("access-control-request-method", "PUT")
                .header(
                    "access-control-request-headers",
                    "authorization,content-type",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(preflight.status().is_success());
    assert!(
        preflight.headers()["access-control-allow-methods"]
            .to_str()
            .unwrap()
            .split(',')
            .any(|value| value.trim() == "PUT")
    );
    assert_eq!(
        preflight.headers()["access-control-allow-origin"],
        "http://127.0.0.1:4329"
    );

    let discovery = request_json(
        app.clone(),
        Method::POST,
        "/discovery/candidates",
        json!({
            "anchors": [],
            "search": "Surface",
            "followed_objects": [],
            "limit": 3,
            "exploration_slots": 1,
            "lens": null
        }),
        Some("http://127.0.0.1:4329"),
    )
    .await;
    assert_eq!(discovery.status, StatusCode::OK);
    assert_eq!(
        discovery
            .header("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("http://127.0.0.1:4329")
    );
    let discovery: DiscoveryResponse = serde_json::from_value(discovery.body).unwrap();
    assert!(!discovery.discovery.objects.is_empty());
    let object = discovery.discovery.objects[0].clone();
    assert!(!object.surfaces.is_empty());

    let prepared = request_json(
        app.clone(),
        Method::POST,
        "/runtime/surfaces/prepare",
        json!({
            "object_id": object.id,
            "role": SurfaceRole::Feed,
        }),
        None,
    )
    .await;
    assert_eq!(prepared.status, StatusCode::OK);
    let prepared: PrepareSurfaceResponse = serde_json::from_value(prepared.body).unwrap();
    assert_eq!(
        prepared.plan.admission,
        babel_runtime::RuntimeAdmissionStatus::Ready
    );
    assert_eq!(
        prepared.plan.surface.target,
        babel_object::SurfaceTarget::Web
    );

    let (_, entry_path_and_query) = prepared
        .plan
        .surface
        .entry
        .split_once("://")
        .expect("absolute Surface entry");
    let entry_path = entry_path_and_query
        .find('/')
        .map(|index| &entry_path_and_query[index..])
        .expect("Surface entry path");
    let fetched = request_json(app, Method::GET, entry_path, Value::Null, None).await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(
        fetched
            .header("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/html")
    );
    assert_eq!(
        fetched
            .header("content-security-policy")
            .and_then(|value| value.to_str().ok()),
        Some(
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"
        )
    );
    assert_eq!(
        fetched
            .header("x-content-type-options")
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
    assert!(
        fetched
            .body
            .as_str()
            .expect("surface HTML body")
            .contains("Babel Object Surface")
    );

    fs::remove_dir_all(root).unwrap();
}

async fn request_json(
    app: Router,
    method: Method,
    uri: &str,
    body: Value,
    origin: Option<&str>,
) -> TestResponse {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    let request = if body == Value::Null {
        builder.body(Body::empty()).unwrap()
    } else {
        builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    };
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    TestResponse {
        status,
        headers,
        body: serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8(body.to_vec()).unwrap())),
    }
}

struct TestResponse {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl TestResponse {
    fn header(&self, name: &str) -> Option<&axum::http::HeaderValue> {
        self.headers.get(name)
    }
}

fn unique_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("babel-api-server-{name}-{nanos}"))
}
