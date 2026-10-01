use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babel_api::{
    ApiState,
    provider::{JudgmentConfig, ServerProvider},
    router,
};
use babel_identity::IdentityKind;
use babel_judgment_python::{PythonProvider, WorkerConfig};
use babel_node::LocalNode;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tower::ServiceExt;

#[tokio::test]
async fn real_python_discovery_provenance_and_sanitized_outages_cross_the_http_boundary() {
    for fault in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "babel-ranking-api-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let provider = if fault {
            ServerProvider::Python(Arc::new(
                PythonProvider::new(WorkerConfig {
                    executable: workspace.join("algorithms/.venv/bin/python"),
                    args: vec![
                        "-I".into(),
                        workspace
                            .join("backend/crates/judgment-python/tests/ranking_worker.py")
                            .display()
                            .to_string(),
                        "error".into(),
                    ],
                    working_directory: None,
                    timeout: Duration::from_secs(3),
                })
                .unwrap(),
            ))
        } else {
            JudgmentConfig::default().start().unwrap()
        };
        let mut node =
            LocalNode::open_with_ranker(&root, provider.clone(), Box::new(provider)).unwrap();
        let author = node
            .create_identity(IdentityKind::Person, "ranking-author")
            .unwrap();
        for text in [
            "According to a study the dataset supports the protocol.",
            "However replication contradicts this public observation.",
        ] {
            node.publish_text(&author.id, text).unwrap();
        }
        let app = router(ApiState::new(node));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let health: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await.unwrap()).unwrap();
        assert_eq!(
            health["ranking_provider"],
            json!({"provider":"babel-python", "model":"lenses-v1", "version":"1"})
        );
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/discovery/candidates")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "anchors": [], "search": null, "followed_objects": [],
                            "limit": 2, "exploration_slots": 1, "lens": null,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        if fault {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("PRIVATE-CONTENT"));
            assert!(!text.contains("traceback"));
            assert!(body.get("discovery").is_none());
        } else {
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(
                body["discovery"]["ranking_provider"],
                health["ranking_provider"]
            );
            assert_eq!(body["discovery"]["ranked"].as_array().unwrap().len(), 2);
            assert_eq!(body["discovery"]["objects"].as_array().unwrap().len(), 2);
        }
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
