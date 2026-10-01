use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babble_api::{ApiState, provider::ServerProvider, router};
use babble_discovery::{NativeTemporalScorer, TemporalProvider, TemporalRequest};
use babble_identity::IdentityKind;
use babble_judgment_python::{PythonProvider, WorkerConfig};
use babble_node::LocalNode;
use babble_object::Object;
use babble_rpc::{RpcBinding, RpcRequestEnvelope, babble_rpc_catalog};
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

// The same acknowledged observer used by temporal.rs, without fault injection.
// Every response comes from the installed production Python executor.
const OBSERVED_WORKER: &str = r#"
import json, os, socket, sys
from dataclasses import asdict
from babble_algorithms.execution import AlgorithmExecutor
from babble_algorithms.worker import handle
executor = AlgorithmExecutor()
observer = socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=3)
for line in sys.stdin.buffer:
    request = json.loads(line)
    response = asdict(handle(line, executor))
    observer.sendall(json.dumps({'pid': os.getpid(), 'request': request, 'response': response}).encode() + b'\n')
    assert observer.recv(16) == b'observed'
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
            "babble-search-api-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
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

    fn corpus(&self) -> Corpus {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let provider = ServerProvider::Python(Arc::new(
            PythonProvider::new(WorkerConfig {
                executable: workspace.join("algorithms/.venv/bin/python"),
                args: vec![
                    "-I".into(),
                    "-c".into(),
                    OBSERVED_WORKER.into(),
                    self.port.to_string(),
                ],
                working_directory: Some(workspace.join("algorithms")),
                timeout: Duration::from_secs(5),
            })
            .unwrap(),
        ));
        let mut node = LocalNode::open_with_algorithms(
            &self.root,
            provider.clone(),
            Box::new(provider.clone()),
            Box::new(provider),
        )
        .unwrap();
        let followed = node
            .create_identity(IdentityKind::Person, "followed-author")
            .unwrap();
        let other = node
            .create_identity(IdentityKind::Person, "other-author")
            .unwrap();
        let matching = vec![
            node.publish_text(&followed.id, "Quasar breaking news today")
                .unwrap(),
            node.publish_text(&followed.id, "QUASAR evergreen API reference")
                .unwrap(),
            node.publish_text(&other.id, "Quasar tutorial and guide")
                .unwrap(),
        ];
        let unrelated = vec![
            node.publish_text(&followed.id, "Gardening discussion")
                .unwrap(),
            node.publish_text(&other.id, "Pottery reference").unwrap(),
        ];
        Corpus {
            app: router(ApiState::new(node)),
            matching,
            unrelated,
            followed: followed.id.to_string(),
        }
    }

    fn take_frames(&self) -> Vec<Value> {
        std::mem::take(&mut *self.frames.lock().unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.observer.take().unwrap().join();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Corpus {
    app: Router,
    matching: Vec<Object>,
    unrelated: Vec<Object>,
    followed: String,
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

#[derive(Clone, Copy, Debug)]
enum Transport {
    Http,
    Rpc,
}

async fn discover(app: &Router, transport: Transport, token: Option<&str>, query: Value) -> Value {
    let (uri, body) = match transport {
        Transport::Http => ("/discovery/candidates", query),
        Transport::Rpc => (
            "/rpc",
            json!(
                RpcRequestEnvelope::new(
                    &babble_rpc_catalog().unwrap(),
                    "discovery-search-regression",
                    "babble.discovery.candidates.v1",
                    RpcBinding::host("web-host", "https://babble.test").unwrap(),
                    query,
                )
                .unwrap()
            ),
        ),
    };
    let (status, body) = request(app, "POST", uri, token, body).await;
    assert_eq!(status, StatusCode::OK, "{transport:?}: {body}");
    let result = match transport {
        Transport::Http => body,
        Transport::Rpc => {
            assert!(body["error"].is_null(), "{body}");
            body["result"].clone()
        }
    };
    assert!(result["discovery"].is_object(), "{result}");
    result["discovery"].clone()
}

fn query(c: &Corpus, search: Value, lens: Value, limit: usize) -> Value {
    // Unrelated explicit source hints and exploration must never broaden search.
    json!({"search":search, "lens":lens, "limit":limit, "exploration_slots":limit,
        "anchors":[c.unrelated[0].id], "followed_objects":[c.unrelated[1].id]})
}

async fn public_lenses(app: &Router) -> Vec<Value> {
    let (status, body) = request(app, "GET", "/lenses", None, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut lenses = vec![Value::Null];
    lenses.extend(body["lenses"].as_array().unwrap().iter()
        .filter(|definition| definition["lens"] != "Following")
        .map(|definition| json!({"id":definition["id"], "weights":[{"lens":definition["lens"], "weight":1.0}]})));
    assert!(lenses.len() > 1, "public lens catalog must be exercised");
    lenses
}

fn ids(values: &Value, key: &str) -> BTreeSet<String> {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value[key].as_str().unwrap().to_owned())
        .collect()
}

fn object_ids(objects: &[Object]) -> BTreeSet<String> {
    objects.iter().map(|object| object.id.to_string()).collect()
}

fn assert_pipeline(
    result: &Value,
    frames: &[Value],
    eligible: &BTreeSet<String>,
    count: usize,
    lens: &Value,
) {
    let objects = result["objects"].as_array().unwrap();
    let ranked = result["ranked"].as_array().unwrap();
    let scores = result["temporal"]["scores"].as_array().unwrap();
    assert_eq!(objects.len(), count, "{result}");
    assert_eq!(ranked.len(), count);
    assert_eq!(scores.len(), count);
    let returned = ids(&result["objects"], "id");
    assert_eq!(returned.len(), count, "duplicate Objects");
    assert!(returned.is_subset(eligible), "unrelated Objects: {result}");
    if count == eligible.len() {
        assert_eq!(&returned, eligible);
    }
    assert_eq!(
        result["ranking_provider"],
        json!({"provider":"babble-python", "model":"lenses-v1", "version":"1"})
    );
    assert_eq!(
        result["temporal"]["provider"],
        json!({"provider":"babble-python", "model":"temporal-v1", "version":"1"})
    );
    let ranks: Vec<_> = frames
        .iter()
        .filter(|frame| frame["request"]["method"] == "rank")
        .collect();
    assert_eq!(ranks.len(), 1, "one real ranking call per discovery");
    let rank_request = &ranks[0]["request"]["request"];
    assert_eq!(&ids(&rank_request["candidates"], "object_id"), eligible);
    assert!(ranks[0]["response"]["error"].is_null());
    assert_eq!(result["ranked"], ranks[0]["response"]["result"]["ranked"]);
    if !lens.is_null() {
        assert_eq!(rank_request["lens"], *lens);
        assert_eq!(result["trace"]["stack_id"], lens["id"]);
    }
    let temporal: Vec<_> = frames
        .iter()
        .filter(|frame| frame["request"]["method"] == "temporal")
        .collect();
    assert_eq!(temporal.len(), usize::from(!eligible.is_empty()));
    if eligible.is_empty() {
        assert!(
            frames
                .iter()
                .all(|frame| frame["request"]["method"] == "rank"),
            "zero matches must not score unrelated Objects"
        );
        return;
    }
    let input = &temporal[0]["request"]["request"];
    assert_eq!(&ids(&input["items"], "object_id"), eligible);
    assert_eq!(
        input["reference_time"],
        result["temporal"]["reference_time"]
    );
    assert!(temporal[0]["response"]["error"].is_null());
    let typed: TemporalRequest = serde_json::from_value(input.clone()).unwrap();
    let canonical = NativeTemporalScorer.score(&typed).unwrap();
    let observed = temporal[0]["response"]["result"]["scores"]
        .as_array()
        .unwrap();
    for ((object, rank), score) in objects.iter().zip(ranked).zip(scores) {
        assert_eq!(object["id"], rank["candidate"]["object_id"]);
        assert_eq!(object["id"], score["object_id"]);
        assert_eq!(
            rank["candidate"]["signals"]["temporal"],
            score["survival_score"]
        );
        assert_eq!(
            score,
            observed
                .iter()
                .find(|s| s["object_id"] == object["id"])
                .unwrap()
        );
        let expected = json!(
            canonical
                .scores
                .iter()
                .find(|s| json!(s.object_id) == object["id"])
                .unwrap()
        );
        for field in [
            "age_hours",
            "recency",
            "decay_rate",
            "time_sensitivity",
            "engagement_velocity",
            "survival_score",
        ] {
            assert!(
                (score[field].as_f64().unwrap() - expected[field].as_f64().unwrap()).abs() < 1e-10,
                "canonical {field} mismatch: {score} != {expected}"
            );
        }
    }
}

#[tokio::test]
async fn real_python_http_and_rpc_search_stays_scoped_across_public_lenses_and_limits() {
    let f = Fixture::new();
    let c = f.corpus();
    let eligible = object_ids(&c.matching);
    let lenses = public_lenses(&c.app).await;
    for transport in [Transport::Http, Transport::Rpc] {
        for lens in &lenses {
            for limit in [1, 2, 20] {
                f.take_frames();
                let result = discover(
                    &c.app,
                    transport,
                    None,
                    query(&c, json!("  qUaSaR  "), lens.clone(), limit),
                )
                .await;
                assert_pipeline(&result, &f.take_frames(), &eligible, limit.min(3), lens);
                for object in result["objects"].as_array().unwrap() {
                    let original = c
                        .matching
                        .iter()
                        .find(|o| json!(o.id) == object["id"])
                        .unwrap();
                    assert_eq!(
                        *object,
                        json!(original),
                        "return original canonical Objects"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn real_python_http_and_rpc_zero_matches_never_fall_back_to_anchors_or_exploration() {
    let f = Fixture::new();
    let c = f.corpus();
    for transport in [Transport::Http, Transport::Rpc] {
        for lens in public_lenses(&c.app).await {
            f.take_frames();
            let result = discover(
                &c.app,
                transport,
                None,
                query(&c, json!("zyxqvabsent82720"), lens.clone(), 20),
            )
            .await;
            assert_pipeline(&result, &f.take_frames(), &BTreeSet::new(), 0, &lens);
        }
    }
}

#[tokio::test]
async fn real_python_http_and_rpc_blank_search_preserves_normal_discovery() {
    let f = Fixture::new();
    let c = f.corpus();
    let eligible = c
        .matching
        .iter()
        .chain(&c.unrelated)
        .map(|o| o.id.to_string())
        .collect();
    for transport in [Transport::Http, Transport::Rpc] {
        for search in [
            Value::Null,
            json!(""),
            json!("   "),
            json!("\u{00a0}\u{2003}"),
        ] {
            f.take_frames();
            let result =
                discover(&c.app, transport, None, query(&c, search, Value::Null, 20)).await;
            assert_pipeline(&result, &f.take_frames(), &eligible, 5, &Value::Null);
        }
    }
}

#[tokio::test]
async fn private_following_search_stays_isolated_from_public_python_discovery() {
    let f = Fixture::new();
    let c = f.corpus();
    let (status, account) = request(&c.app, "POST", "/auth/register", None,
        json!({"handle":"private-search-viewer", "kind":"Person", "password":"Private Search account password 9327!"})).await;
    assert_eq!(status, StatusCode::OK, "{account}");
    let token = account["token"].as_str().unwrap();
    let (status, body) = request(
        &c.app,
        "PUT",
        &format!("/social/following/{}", c.followed),
        Some(token),
        json!({"following":true, "expected_revision":0, "idempotency_key":"private-search-follow"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    f.take_frames();
    let mut expected = c.matching[..2].to_vec();
    expected.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    for transport in [Transport::Http, Transport::Rpc] {
        for (suffix, expected_objects) in
            [("quasar", json!(expected)), ("zyxqvabsent82720", json!([]))]
        {
            let (status, feed) = request(
                &c.app,
                "GET",
                &format!("/feed/following?search={suffix}"),
                Some(token),
                Value::Null,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{feed}");
            assert_eq!(feed["objects"], expected_objects);
            assert!(
                f.take_frames().is_empty(),
                "Following must bypass all algorithms"
            );
        }
        let result = discover(
            &c.app,
            transport,
            Some(token),
            query(&c, json!("quasar"), Value::Null, 20),
        )
        .await;
        let frames = f.take_frames();
        assert_pipeline(&result, &frames, &object_ids(&c.matching), 3, &Value::Null);
        for candidate in result["ranked"].as_array().unwrap() {
            assert_eq!(
                candidate["candidate"]["signals"]["followed_author"], false,
                "private person follows must not become public ranking signals"
            );
        }
        let serialized = serde_json::to_string(&frames).unwrap();
        for secret in [
            token,
            account["identity"]["id"].as_str().unwrap(),
            "private-search-viewer",
            "private-search-follow",
            "Private Search account password 9327!",
        ] {
            assert!(
                !serialized.contains(secret),
                "private Following data reached worker"
            );
        }
    }
}
