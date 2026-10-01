use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babble_api::{
    ApiState,
    provider::{JudgmentConfig, ServerProvider},
    router,
};
use babble_discovery::{NativeTemporalScorer, TemporalProvider, TemporalRequest};
use babble_identity::IdentityKind;
use babble_judgment_python::{PythonProvider, WorkerConfig};
use babble_node::LocalNode;
use babble_types::Timestamp;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use tower::ServiceExt;

// Delegate every operation to the installed worker. Mutations happen only after
// a real temporal response, so failures cannot pass by breaking another method.
const OBSERVED_WORKER: &str = r#"
import json, os, socket, sys
from dataclasses import asdict
from babble_algorithms.execution import AlgorithmExecutor
from babble_algorithms.worker import handle
executor = AlgorithmExecutor()
mode, port = sys.argv[1:]
observer = socket.create_connection(('127.0.0.1', int(port)), timeout=3)
for line in sys.stdin.buffer:
    request = json.loads(line)
    response = asdict(handle(line, executor))
    observer.sendall(json.dumps({'pid': os.getpid(), 'request': request, 'response': response}).encode() + b'\n')
    assert observer.recv(16) == b'observed'
    if request['method'] == 'temporal':
        result = response['result']
        if mode == 'error':
            response['result'] = None
            response['error'] = {'code': 'algorithm_failure', 'message': 'PRIVATE-TEMPORAL traceback'}
        elif mode == 'exit':
            sys.stderr.write('PRIVATE-TEMPORAL traceback\n')
            sys.exit(17)
        elif mode == 'provider':
            result['provider']['version'] = '999'
        elif mode == 'missing':
            result['scores'].pop()
        elif mode == 'duplicate':
            result['scores'][1] = result['scores'][0]
        elif mode == 'order':
            result['scores'].reverse()
        elif mode == 'foreign':
            result['scores'][0]['object_id'] = 'obj_' + 'f' * 64
        elif mode == 'time':
            result['reference_time'] = '2000-01-01T00:00:00Z'
        elif mode == 'score':
            result['scores'][0]['survival_score'] = 1.5
        elif mode == 'extra':
            result['private_history'] = ['PRIVATE-TEMPORAL']
    print(json.dumps(response, separators=(',', ':')), flush=True)
"#;

struct Fixture {
    root: PathBuf,
    port: u16,
    frames: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    observer: Option<JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-temporal-api-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        // The worker forbids file writes. Acknowledged loopback messages keep
        // observations in memory and make assertions independent of scheduling.
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let port = socket.local_addr().unwrap().port();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let captured = frames.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let observer = std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match socket.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_millis(100)))
                            .unwrap();
                        let mut reader = BufReader::new(stream);
                        let mut line = String::new();
                        while !stopping.load(Ordering::Relaxed) {
                            match reader.read_line(&mut line) {
                                Ok(0) => break,
                                Ok(_) => {
                                    captured
                                        .lock()
                                        .unwrap()
                                        .push(serde_json::from_str(&line).unwrap());
                                    reader.get_mut().write_all(b"observed").unwrap();
                                    line.clear();
                                }
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        std::io::ErrorKind::WouldBlock
                                            | std::io::ErrorKind::TimedOut
                                    ) => {}
                                Err(error) => panic!("worker observer: {error}"),
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("worker observer: {error}"),
                }
            }
        });
        Self {
            root,
            port,
            frames,
            stop,
            observer: Some(observer),
        }
    }
    fn observed_provider(&self, mode: &str) -> ServerProvider {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        ServerProvider::Python(Arc::new(
            PythonProvider::new(WorkerConfig {
                executable: workspace.join("algorithms/.venv/bin/python"),
                args: vec![
                    "-I".into(),
                    "-c".into(),
                    OBSERVED_WORKER.into(),
                    mode.into(),
                    self.port.to_string(),
                ],
                working_directory: Some(workspace.join("algorithms")),
                timeout: Duration::from_secs(5),
            })
            .unwrap(),
        ))
    }
    fn node(&self, provider: ServerProvider) -> LocalNode<ServerProvider> {
        LocalNode::open_with_algorithms(
            self.root.join("store"),
            provider.clone(),
            Box::new(provider.clone()),
            Box::new(provider),
        )
        .unwrap()
    }
    fn frames(&self) -> Vec<Value> {
        self.frames.lock().unwrap().clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.observer.take().unwrap().join();
        let _ = std::fs::remove_dir_all(&self.root);
    }
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
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn discovery() -> Value {
    json!({"anchors":[], "search":null, "followed_objects":[], "limit":2, "exploration_slots":1, "lens":null})
}

fn assert_mapping(body: &Value, health: &Value) {
    let result = &body["discovery"];
    assert_eq!(result["temporal"]["provider"], health["temporal_provider"]);
    assert_eq!(result["ranking_provider"], health["ranking_provider"]);
    let ranked = result["ranked"].as_array().unwrap();
    let objects = result["objects"].as_array().unwrap();
    let scores = result["temporal"]["scores"].as_array().unwrap();
    assert_eq!(ranked.len(), 2);
    assert_eq!(objects.len(), ranked.len());
    assert_eq!(scores.len(), ranked.len());
    for ((rank, object), score) in ranked.iter().zip(objects).zip(scores) {
        assert_eq!(rank["candidate"]["object_id"], object["id"]);
        assert_eq!(score["object_id"], object["id"]);
        assert_eq!(
            rank["candidate"]["signals"]["temporal"],
            score["survival_score"]
        );
    }
}

#[tokio::test]
async fn default_real_python_and_explicit_rust_local_report_temporal_provenance_through_http() {
    for (config, expected) in [
        (JudgmentConfig::default(), "babble-python"),
        (JudgmentConfig::RustLocal, "babble-rust"),
    ] {
        let f = Fixture::new();
        let mut node = f.node(config.start().unwrap());
        let author = node
            .create_identity(IdentityKind::Person, "temporal-author")
            .unwrap();
        for text in [
            "Breaking public news today",
            "Evergreen API reference",
            "A public discussion",
        ] {
            node.publish_text(&author.id, text).unwrap();
        }
        let app = router(ApiState::new(node));
        let (status, health) = request(&app, "GET", "/health", None, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            health["temporal_provider"],
            json!({"provider":expected, "model":"temporal-v1", "version":"1"})
        );
        let before = Timestamp::now();
        let (status, body) =
            request(&app, "POST", "/discovery/candidates", None, discovery()).await;
        let after = Timestamp::now();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_mapping(&body, &health);
        let reference: Timestamp =
            serde_json::from_value(body["discovery"]["temporal"]["reference_time"].clone())
                .unwrap();
        assert!(reference >= before && reference <= after);
        drop(app);
    }
}

#[tokio::test]
async fn temporal_worker_faults_and_malformed_responses_return_sanitized_503_without_fallback() {
    for mode in [
        "error",
        "exit",
        "provider",
        "missing",
        "duplicate",
        "order",
        "foreign",
        "time",
        "score",
        "extra",
    ] {
        let f = Fixture::new();
        let mut node = f.node(f.observed_provider(mode));
        let author = node
            .create_identity(IdentityKind::Person, "temporal-author")
            .unwrap();
        node.publish_text(&author.id, "First public discussion")
            .unwrap();
        node.publish_text(&author.id, "Second public discussion")
            .unwrap();
        let app = router(ApiState::new(node));
        let (status, body) =
            request(&app, "POST", "/discovery/candidates", None, discovery()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{mode}: {body}");
        assert!(body.get("discovery").is_none(), "{mode}: {body}");
        for secret in [
            "PRIVATE-TEMPORAL",
            "traceback",
            "worker-frames",
            f.root.to_str().unwrap(),
        ] {
            assert!(!body.to_string().contains(secret), "{mode}: {body}");
        }
        let frames = f.frames();
        let temporal: Vec<_> = frames
            .iter()
            .filter(|f| f["request"]["method"] == "temporal")
            .collect();
        assert_eq!(temporal.len(), 1, "{mode}");
        assert!(
            temporal[0]["response"]["error"].is_null(),
            "real worker failed before injection: {mode}"
        );
        assert!(
            temporal[0]["response"]["result"]["scores"].is_array(),
            "{mode}"
        );
        assert!(
            !frames.iter().any(|f| f["request"]["method"] == "rank"),
            "ranking after temporal failure: {mode}"
        );
        drop(app);
    }
}

#[tokio::test]
async fn shared_real_worker_receives_public_temporal_fields_only_and_following_bypasses_it() {
    let f = Fixture::new();
    let mut node = f.node(f.observed_provider("valid"));
    let author = node
        .create_identity(IdentityKind::Person, "public-author")
        .unwrap();
    let first = node
        .publish_text(&author.id, "First public discussion")
        .unwrap();
    let second = node
        .publish_text(&author.id, "Second public discussion")
        .unwrap();
    let app = router(ApiState::new(node));
    let password = "Private Temporal account password 9327!";
    let (status, account) = request(
        &app,
        "POST",
        "/auth/register",
        None,
        json!({"handle":"private-temporal-viewer", "kind":"Person", "password":password}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    let token = account["token"].as_str().unwrap();
    let private_id = account["identity"]["id"].as_str().unwrap();
    let (status, body) = request(
        &app,
        "PUT",
        &format!("/social/following/{}", author.id),
        Some(token),
        json!({"following":true, "expected_revision":0, "idempotency_key":"private-follow-marker"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before_following = f.frames();
    let (status, feed) = request(
        &app,
        "GET",
        "/feed/following?limit=20",
        Some(token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert_eq!(feed["objects"][0]["id"], json!(second.id));
    assert_eq!(feed["objects"][1]["id"], json!(first.id));
    assert_eq!(
        f.frames(),
        before_following,
        "Following must not invoke any algorithm"
    );
    let (status, health) = request(&app, "GET", "/health", None, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = request(
        &app,
        "POST",
        "/discovery/candidates",
        Some(token),
        discovery(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_mapping(&body, &health);
    let frames = f.frames();
    let pids: BTreeSet<_> = frames.iter().map(|f| f["pid"].as_u64().unwrap()).collect();
    assert_eq!(
        pids.len(),
        1,
        "all three providers must share one worker process"
    );
    for method in ["judge", "rank", "temporal"] {
        assert!(
            frames.iter().any(|f| f["request"]["method"] == method),
            "missing real {method} call"
        );
    }
    let serialized = serde_json::to_string(&frames).unwrap();
    for secret in [
        password,
        token,
        private_id,
        "private-temporal-viewer",
        "private-follow-marker",
        "telemetry",
        "password_hash",
    ] {
        assert!(
            !serialized.contains(secret),
            "private value reached worker: {secret}"
        );
    }
    let temporal: Vec<_> = frames
        .iter()
        .filter(|f| f["request"]["method"] == "temporal")
        .collect();
    assert_eq!(temporal.len(), 1);
    let input = &temporal[0]["request"]["request"];
    assert_eq!(
        input
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["items", "reference_time"])
    );
    let typed: TemporalRequest = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(typed.items.len(), 2);
    for item in &typed.items {
        assert_eq!(item.engagement.total_views, 0);
        assert_eq!(item.engagement.recent_views, 0);
        assert_eq!(
            item.engagement.total_interactions, 0,
            "private person follows are not public activity"
        );
    }
    let expected = NativeTemporalScorer.score(&typed).unwrap();
    for score in body["discovery"]["temporal"]["scores"].as_array().unwrap() {
        let native = expected
            .scores
            .iter()
            .find(|s| json!(s.object_id) == score["object_id"])
            .unwrap();
        assert!((score["survival_score"].as_f64().unwrap() - native.survival_score).abs() < 1e-12);
    }
    drop(app);
}
