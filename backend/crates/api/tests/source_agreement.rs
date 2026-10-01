use axum::http::StatusCode;
use babel_api::{
    ApiState, JudgeObjectResponse,
    config::ServerConfig,
    provider::{JudgmentConfig, ServerProvider},
    serve::configured_router,
};
use babel_crypto::Keypair;
use babel_graph::{Edge, EdgeOrigin, Relation};
use babel_identity::{Identity, IdentityKind};
use babel_judgment::{JudgmentProvider, SourceAgreementInput, SourceAgreementOutput};
use babel_judgment_python::{PythonProvider, WorkerConfig};
use babel_node::{ImportBundle, LocalNode};
use babel_object::{Object, ObjectKind};
use babel_rpc::{RpcBinding, RpcRequestEnvelope, babel_rpc_catalog};
use babel_types::{ObjectId, Timestamp};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
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

const DEFINITION: &str = "babel.judgment.source_agreement.v1";
const PRIVATE: &str = "PRIVATE-SOURCE-AGREEMENT-DO-NOT-EXPORT";

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "babel-source-agreement-api-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn node(&self, provider: ServerProvider) -> LocalNode<ServerProvider> {
        LocalNode::open_with_algorithms(
            self.0.join("store"),
            provider.clone(),
            Box::new(provider.clone()),
            Box::new(provider),
        )
        .unwrap()
    }
    fn python(&self) -> LocalNode<ServerProvider> {
        self.node(JudgmentConfig::default().start().unwrap())
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Api {
    origin: String,
    client: reqwest::Client,
    task: tokio::task::JoinHandle<()>,
}
impl Api {
    async fn start(root: &Root, node: LocalNode<ServerProvider>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let origin = format!("http://{address}");
        let app = configured_router(
            ServerConfig {
                bind_addr: address,
                public_origin: origin.clone(),
                store_root: root.0.join("store"),
                seed_profile: None,
                cors_origins: vec![],
                judgment: JudgmentConfig::default(),
                bundle_gateway: None,
            },
            ApiState::new(node),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            origin,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            task,
        }
    }
    async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Value,
    ) -> (StatusCode, Value) {
        let mut request = self
            .client
            .request(method.parse().unwrap(), format!("{}{path}", self.origin));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if !body.is_null() {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let bytes = response.bytes().await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)})),
        )
    }
    async fn get(&self, path: &str) -> Value {
        ok(self.request("GET", path, None, Value::Null).await)
    }
    async fn register(&self) -> String {
        ok(self.request("POST", "/auth/register", None, json!({
            "handle":"agreement-viewer", "kind":"Person", "password":format!("{PRIVATE}-password-93!"),
        })).await)["token"].as_str().unwrap().to_owned()
    }
    async fn evaluate(&self, id: &ObjectId, token: &str) -> Value {
        ok(self
            .request(
                "POST",
                &format!("/judgments/object/{id}"),
                Some(token),
                json!({"definition":DEFINITION,"parameters":{}}),
            )
            .await)
    }
    async fn rpc(&self, method: &str, token: &str, params: Value) -> (StatusCode, Value) {
        let envelope = RpcRequestEnvelope::new(
            &babel_rpc_catalog().unwrap(),
            "agreement-test",
            method,
            RpcBinding::host("agreement-tests", &self.origin).unwrap(),
            params,
        )
        .unwrap();
        self.request(
            "POST",
            "/rpc",
            Some(token),
            serde_json::to_value(envelope).unwrap(),
        )
        .await
    }
    async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}
impl Drop for Api {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn ok(response: (StatusCode, Value)) -> Value {
    assert_eq!(response.0, StatusCode::OK, "{}", response.1);
    response.1
}
fn rpc_ok(response: (StatusCode, Value)) -> Value {
    let response = ok(response);
    assert!(response["error"].is_null(), "{response}");
    response["result"].clone()
}
fn author(node: &mut LocalNode<ServerProvider>) -> (Identity, Keypair) {
    let key = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "agreement-author", &key).unwrap();
    node.import_signing_identity(identity.clone(), key.clone())
        .unwrap();
    (identity, key)
}
fn source(
    node: &mut LocalNode<ServerProvider>,
    author: &Identity,
    key: &Keypair,
    payload: Value,
) -> Object {
    let object = Object::create(
        author,
        ObjectKind::new("test.public_source"),
        "test.public_source.v1",
        payload,
    )
    .unwrap()
    .sign(author, key)
    .unwrap();
    node.publish_object_record(&author.id, object).unwrap()
}
fn edge(
    node: &mut LocalNode<ServerProvider>,
    author: &Identity,
    from: &Object,
    to: &Object,
    relation: Relation,
) {
    let edge = node
        .publish_edge(
            &author.id,
            from.id.clone(),
            to.id.clone(),
            relation,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    edge.verify(author).unwrap();
}
fn association(
    value: &Value,
    object: &ObjectId,
) -> (
    JudgeObjectResponse,
    SourceAgreementInput,
    SourceAgreementOutput,
) {
    let response: JudgeObjectResponse = serde_json::from_value(value.clone()).unwrap();
    let input = response.input.as_ref().expect("durable input association");
    assert_eq!(&input.object_id, object);
    assert_eq!(input.judgment_id, response.judgment.id);
    assert_eq!(input.request.definition.as_str(), DEFINITION);
    assert!(input.request.parameters.is_empty());
    assert_eq!(input.request.state.subject, object.to_string());
    let request = SourceAgreementInput::from_request(&input.request).unwrap();
    let output: SourceAgreementOutput =
        serde_json::from_value(response.judgment.output.clone()).unwrap();
    output.validate_for(&input.request).unwrap();
    assert_eq!(response.judgment.provider.provider, "babel-python");
    assert_eq!(response.judgment.confidence, 0.0);
    assert_eq!(output.confidence, 0.0);
    assert_eq!(output.confidence_status, "uncalibrated");
    assert!(output.user_contributions.is_empty());
    assert!(
        request
            .sources
            .iter()
            .all(|s| s.vote.is_none() && s.user_id.is_none())
    );
    (response, request, output)
}
async fn history(api: &Api, object: &ObjectId) -> Vec<Value> {
    api.get(&format!("/objects/{object}/judgments")).await["judgments"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn signed_sources_python_components_http_rpc_readback_restart_and_new_history() {
    let root = Root::new();
    let mut node = root.python();
    let (author, key) = author(&mut node);
    let claim = node
        .publish_text(&author.id, "The dataset reports a measured improvement.")
        .unwrap();
    let mut ids = BTreeSet::new();
    let mut context_ids = BTreeSet::new();
    for (relation, text) in [
        (
            Relation::Supports,
            "According to a study, the dataset and methodology support a measured improvement of 25 percent.",
        ),
        (
            Relation::Contradicts,
            "However, replication contradicts the claim: the measured improvement was not observed.",
        ),
        (
            Relation::EvidenceFor,
            "The original experiment reports a dataset with a measured improvement and a reproducible methodology.",
        ),
        (
            Relation::EvidenceAgainst,
            "A replication study found no measured improvement in the comparison dataset.",
        ),
        (
            Relation::References,
            "A background glossary describes the experiment, dataset and statistical terminology.",
        ),
        (
            Relation::Cites,
            "The public methods appendix explains measurement calibration and sample selection.",
        ),
    ] {
        let object = node.publish_text(&author.id, text).unwrap();
        object.verify(&author).unwrap();
        if matches!(relation, Relation::References | Relation::Cites) {
            context_ids.insert(object.id.to_string());
        }
        ids.insert(object.id.to_string());
        edge(&mut node, &author, &object, &claim, relation);
    }
    let api = Api::start(&root, node).await;
    let token = api.register().await;
    let health = api.get("/health").await;
    assert_eq!(health["judgment_provider"]["provider"], "babel-python");
    let providers = api.get("/judgments/providers").await;
    let supported = providers["providers"][0]["supported_definitions"]
        .as_array()
        .unwrap();
    assert_eq!(supported.len(), 7);
    assert!(supported.contains(&json!(DEFINITION)));
    let definitions = api.get("/judgments/definitions").await;
    assert_eq!(definitions["definitions"].as_array().unwrap().len(), 7);
    let before = history(&api, &claim.id).await;
    assert!(!before.iter().any(|j| j["definition"] == DEFINITION));
    assert!(
        before
            .iter()
            .any(|j| j["definition"] == "babel.judgment.moderation.v1")
    );
    let start = Timestamp::now();
    let first = api.evaluate(&claim.id, &token).await;
    let end = Timestamp::now();
    let (judged, input, output) = association(&first, &claim.id);
    assert!(input.reference_time >= start.0.unix_timestamp() as f64);
    assert!(input.reference_time <= end.0.unix_timestamp() as f64 + 1.0);
    assert!(input.previous_score.is_none());
    assert_eq!(
        output.source_ids.iter().cloned().collect::<BTreeSet<_>>(),
        ids
    );
    assert_eq!(output.validation_count, 6);
    assert_eq!(
        input
            .sources
            .iter()
            .filter(|s| s.is_context)
            .map(|s| s.source_id.clone())
            .collect::<BTreeSet<_>>(),
        context_ids
    );
    for source in &input.sources {
        let source_history = api
            .get(&format!("/objects/{}/judgments", source.source_id))
            .await;
        let records = source_history["judgments"].as_array().unwrap();
        let moderation = records
            .iter()
            .find(|j| j["definition"] == "babel.judgment.moderation.v1")
            .unwrap();
        let evidence = records
            .iter()
            .find(|j| j["definition"] == "babel.judgment.evidence_quality.v1")
            .unwrap();
        assert_eq!(moderation["provider"]["provider"], "babel-python");
        assert_eq!(evidence["provider"]["provider"], "babel-python");
        assert_eq!(
            source.quality_score,
            moderation["output"]["quality"].as_f64().unwrap()
        );
        assert_eq!(
            source.evidence_score,
            evidence["output"]["score"].as_f64().unwrap()
        );
    }
    // Re-evaluate the exact stored request with an independent real Python process.
    let expected = JudgmentConfig::default()
        .start()
        .unwrap()
        .judge(&judged.input.as_ref().unwrap().request)
        .unwrap();
    assert_eq!(expected.output, judged.judgment.output);
    assert_eq!(expected.id, judged.judgment.id);
    let fetched = api.get(&format!("/judgments/{}", judged.judgment.id)).await;
    assert_eq!(fetched["judgment"], first["judgment"]);
    assert_eq!(fetched["input"], first["input"]);
    let rpc = rpc_ok(
        api.rpc(
            "babel.judgment.object.evaluate.v1",
            &token,
            json!({"object_id":claim.id,"definition":DEFINITION,"parameters":{}}),
        )
        .await,
    );
    assert_eq!(rpc["judgment"], first["judgment"]);
    assert_eq!(rpc["input"], first["input"]);
    assert_eq!(rpc["orchestration"]["cache_hit"], true);
    let rpc_list = rpc_ok(
        api.rpc(
            "babel.judgment.object.list.v1",
            &token,
            json!({"object_id":claim.id}),
        )
        .await,
    );
    assert!(
        rpc_list["judgments"]
            .as_array()
            .unwrap()
            .contains(&first["judgment"])
    );
    assert_eq!(history(&api, &claim.id).await.len(), before.len() + 1);
    api.stop().await;

    let api = Api::start(&root, root.python()).await;
    let restarted = api.evaluate(&claim.id, &token).await;
    assert_eq!(
        restarted["judgment"], first["judgment"],
        "restart must preserve created_at as well as ID"
    );
    assert_eq!(restarted["input"], first["input"]);
    assert_eq!(restarted["orchestration"]["cache_hit"], true);
    assert_eq!(history(&api, &claim.id).await.len(), before.len() + 1);
    api.stop().await;

    let mut node = root.python();
    let new_source = source(
        &mut node,
        &author,
        &key,
        json!({"text":"New independent measurement refutes the reported increase using an expanded sample and dataset."}),
    );
    edge(
        &mut node,
        &author,
        &new_source,
        &claim,
        Relation::Contradicts,
    );
    let api = Api::start(&root, node).await;
    let changed = api.evaluate(&claim.id, &token).await;
    let (_, input, output) = association(&changed, &claim.id);
    assert_ne!(changed["judgment"]["id"], first["judgment"]["id"]);
    assert_eq!(
        input.previous_score,
        Some(judged.judgment.output["consensus_score"].as_f64().unwrap())
    );
    assert_eq!(output.validation_count, 7);
    let records = history(&api, &claim.id).await;
    assert_eq!(records.len(), before.len() + 2);
    assert!(records.contains(&first["judgment"]));
    assert!(records.contains(&changed["judgment"]));
    assert_eq!(
        api.get(&format!("/judgments/{}", judged.judgment.id)).await["input"],
        first["input"]
    );
    api.stop().await;
}

#[tokio::test]
async fn exact_copies_duplicate_edges_non_evidence_and_future_records_do_not_add_sources() {
    let root = Root::new();
    let mut node = root.python();
    let (author, key) = author(&mut node);
    let claim = node.publish_text(&author.id, "A public claim").unwrap();
    let original = node
        .publish_text(
            &author.id,
            "The study reports a reproducible dataset and methodology.",
        )
        .unwrap();
    let copied = node
        .publish_text(
            &author.id,
            "The study reports a reproducible dataset and methodology.",
        )
        .unwrap();
    assert_ne!(original.id, copied.id);
    for relation in [Relation::Supports, Relation::EvidenceFor, Relation::Cites] {
        edge(&mut node, &author, &original, &claim, relation);
    }
    edge(&mut node, &author, &copied, &claim, Relation::Supports);
    edge(&mut node, &author, &claim, &claim, Relation::Supports);
    let unrelated = node
        .publish_text(&author.id, "Excluded non-evidence text")
        .unwrap();
    edge(&mut node, &author, &unrelated, &claim, Relation::ReplyTo);
    edge(&mut node, &author, &claim, &unrelated, Relation::Supports);
    for origin in [EdgeOrigin::JudgmentDerived, EdgeOrigin::ConsensusDerived] {
        node.publish_edge(
            &author.id,
            unrelated.id.clone(),
            claim.id.clone(),
            Relation::Supports,
            origin,
        )
        .unwrap();
    }
    let unsigned = Edge::new(
        unrelated.id.clone(),
        claim.id.clone(),
        Relation::Supports,
        EdgeOrigin::HumanAssertion,
        Some(author.id.clone()),
    )
    .unwrap();
    assert!(
        node.import_bundle(ImportBundle {
            edges: vec![unsigned],
            ..Default::default()
        })
        .is_err()
    );
    let mut future = Object::text(&author, "A future source is not current evidence").unwrap();
    future.created_at = Timestamp(Timestamp::now().0 + time::Duration::days(1));
    future = future
        .with_state(json!({}))
        .unwrap()
        .sign(&author, &key)
        .unwrap();
    node.import_bundle(ImportBundle {
        objects: vec![future.clone()],
        ..Default::default()
    })
    .unwrap();
    let mut future_edge = Edge::new(
        future.id.clone(),
        claim.id.clone(),
        Relation::Supports,
        EdgeOrigin::HumanAssertion,
        Some(author.id.clone()),
    )
    .unwrap();
    future_edge.created_at = Timestamp(future.created_at.0 + time::Duration::seconds(1));
    future_edge = future_edge
        .with_metadata(BTreeMap::new())
        .unwrap()
        .sign(&author, &key)
        .unwrap();
    node.import_bundle(ImportBundle {
        edges: vec![future_edge],
        ..Default::default()
    })
    .unwrap();
    // A future edge on a present object must also be ignored.
    let mut future_edge = Edge::new(
        unrelated.id.clone(),
        claim.id.clone(),
        Relation::EvidenceFor,
        EdgeOrigin::HumanAssertion,
        Some(author.id.clone()),
    )
    .unwrap();
    future_edge.created_at = future.created_at;
    future_edge = future_edge
        .with_metadata(BTreeMap::new())
        .unwrap()
        .sign(&author, &key)
        .unwrap();
    node.import_bundle(ImportBundle {
        edges: vec![future_edge],
        ..Default::default()
    })
    .unwrap();
    let api = Api::start(&root, node).await;
    let token = api.register().await;
    let value = api.evaluate(&claim.id, &token).await;
    let (_, input, output) = association(&value, &claim.id);
    assert_eq!(output.validation_count, 1);
    assert!([original.id.to_string(), copied.id.to_string()].contains(&input.sources[0].source_id));
    assert!(!input.sources[0].is_context);
    api.stop().await;
}

#[tokio::test]
async fn empty_evidence_is_insufficient_and_http_rpc_caller_forgery_is_rejected() {
    let root = Root::new();
    let mut node = root.python();
    let (author, _) = author(&mut node);
    let claim = node
        .publish_text(&author.id, "No signed source has been attached.")
        .unwrap();
    let api = Api::start(&root, node).await;
    let token = api.register().await;
    let before = history(&api, &claim.id).await;
    for parameters in [
        json!({"reference_time":0}),
        json!({"sources":[]}),
        json!({"previous_score":1}),
        json!({"private_token":PRIVATE}),
    ] {
        let rejected = api
            .request(
                "POST",
                &format!("/judgments/object/{}", claim.id),
                Some(&token),
                json!({"definition":DEFINITION,"parameters":parameters}),
            )
            .await;
        assert!(rejected.0.is_client_error(), "{rejected:?}");
        let rejected = ok(api
            .rpc(
                "babel.judgment.object.evaluate.v1",
                &token,
                json!({"object_id":claim.id,"definition":DEFINITION,"parameters":parameters}),
            )
            .await);
        assert!(!rejected["error"].is_null(), "{rejected}");
        assert!(rejected["result"].is_null());
    }
    assert_eq!(history(&api, &claim.id).await, before);
    let result = api.evaluate(&claim.id, &token).await;
    let (_, input, output) = association(&result, &claim.id);
    assert!(input.sources.is_empty());
    assert_eq!(output.validation_count, 0);
    assert_eq!(result["judgment"]["output"]["state"], "insufficient");
    assert_eq!(output.consensus_score, 0.0);
    assert_eq!(output.reliability_score, 0.0);
    assert_eq!(history(&api, &claim.id).await.len(), before.len() + 1);
    api.stop().await;
}

// Every frame is produced by the real executor. Faults modify only its finished
// source-agreement result; acknowledged loopback capture proves the call occurred.
const OBSERVED_WORKER: &str = r#"
import json, socket, sys
from dataclasses import asdict
from babel_algorithms.execution import AlgorithmExecutor
from babel_algorithms.worker import handle
executor = AlgorithmExecutor()
mode, port = sys.argv[1:]
observer = socket.create_connection(('127.0.0.1', int(port)), timeout=3)
for line in sys.stdin.buffer:
    request = json.loads(line)
    response = asdict(handle(line, executor))
    observer.sendall(json.dumps({'request': request, 'response': response}).encode() + b'\n')
    assert observer.recv(16) == b'observed'
    if request['method'] == 'judge' and request['request']['definition'] == 'babel.judgment.source_agreement.v1':
        assert response['error'] is None
        result = response['result']
        output = result['output']
        if mode == 'error':
            response['result'] = None
            response['error'] = {'code': 'algorithm_failure', 'message': 'PRIVATE-SOURCE-AGREEMENT-DO-NOT-EXPORT traceback'}
        elif mode == 'exit':
            sys.stderr.write('PRIVATE-SOURCE-AGREEMENT-DO-NOT-EXPORT traceback\n')
            sys.exit(17)
        elif mode == 'provider': result['provider']['version'] = '999'
        elif mode == 'confidence': output['confidence'] = result['confidence'] = 0.9
        elif mode == 'score': output['consensus_score'] = 1.5
        elif mode == 'foreign': output['source_ids'][0] = 'foreign-source'
        elif mode == 'count': output['validation_count'] += 1
        elif mode == 'time': output['reference_time'] -= 1
        elif mode == 'extra': output['private_raw'] = 'PRIVATE-SOURCE-AGREEMENT-DO-NOT-EXPORT'
        elif mode == 'voter': output['user_contributions'] = {'fabricated-user': 0.5}
        elif mode == 'missing': del output['fact_agreement']
    print(json.dumps(response, separators=(',', ':')), flush=True)
"#;

struct Observer {
    port: u16,
    frames: Arc<Mutex<Vec<Value>>>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Observer {
    fn new() -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        socket.set_nonblocking(true).unwrap();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let capture = frames.clone();
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match socket.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_millis(100)))
                            .unwrap();
                        let mut reader = BufReader::new(stream);
                        let mut line = String::new();
                        while !stop.load(Ordering::Relaxed) {
                            match reader.read_line(&mut line) {
                                Ok(0) => break,
                                Ok(_) => {
                                    capture
                                        .lock()
                                        .unwrap()
                                        .push(serde_json::from_str(&line).unwrap());
                                    reader.get_mut().write_all(b"observed").unwrap();
                                    line.clear();
                                }
                                Err(e)
                                    if matches!(
                                        e.kind(),
                                        std::io::ErrorKind::WouldBlock
                                            | std::io::ErrorKind::TimedOut
                                    ) => {}
                                Err(e) => panic!("worker observer: {e}"),
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("worker observer: {e}"),
                }
            }
        });
        Self {
            port,
            frames,
            stopping,
            thread: Some(thread),
        }
    }
    fn provider(&self, mode: &str) -> ServerProvider {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../algorithms");
        ServerProvider::Python(Arc::new(
            PythonProvider::new(WorkerConfig {
                executable: directory.join(".venv/bin/python"),
                args: vec![
                    "-I".into(),
                    "-c".into(),
                    OBSERVED_WORKER.into(),
                    mode.into(),
                    self.port.to_string(),
                ],
                working_directory: Some(directory),
                timeout: Duration::from_secs(5),
            })
            .unwrap(),
        ))
    }
    fn frames(&self) -> Vec<Value> {
        self.frames.lock().unwrap().clone()
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[tokio::test]
async fn only_public_source_fields_reach_real_python_and_persisted_inputs() {
    let root = Root::new();
    let observer = Observer::new();
    let mut node = root.node(observer.provider("valid"));
    let (author, key) = author(&mut node);
    let claim = node.publish_text(&author.id, "Public claim").unwrap();
    let public = Object::create(
        &author,
        ObjectKind::new("test.public_source"),
        "test.public_source.v1",
        json!({
            "text":"Public source text", "title":"Public title", "description":"Public description", "summary":"Public summary",
            "private":PRIVATE,"metadata":{"private":PRIVATE},"session_token":PRIVATE,"raw":PRIVATE,
        }),
    ).unwrap().sign(&author, &key).unwrap();
    node.import_bundle(ImportBundle {
        objects: vec![public.clone()],
        ..Default::default()
    })
    .unwrap();
    edge(&mut node, &author, &public, &claim, Relation::EvidenceFor);
    let api = Api::start(&root, node).await;
    let token = api.register().await;
    let before_evaluation = observer.frames().len();
    let result = api.evaluate(&claim.id, &token).await;
    let (_, input, _) = association(&result, &claim.id);
    assert_eq!(
        input.sources[0].text,
        "Public source text\nPublic title\nPublic description\nPublic summary"
    );
    let frames = observer.frames();
    let agreement = frames
        .iter()
        .find(|frame| frame["request"]["request"]["definition"] == DEFINITION)
        .unwrap();
    assert!(agreement["response"]["error"].is_null());
    assert_eq!(agreement["request"]["request"], result["input"]["request"]);
    assert_eq!(
        agreement["response"]["result"]["output"],
        result["judgment"]["output"]
    );
    let serialized = serde_json::to_string(&frames[before_evaluation..]).unwrap();
    for secret in [PRIVATE, token.as_str(), "session_token", "password_hash"] {
        assert!(!serialized.contains(secret), "private input reached worker");
        assert!(
            !result["input"].to_string().contains(secret),
            "private input persisted"
        );
    }
    api.stop().await;
}

#[tokio::test]
async fn real_worker_failures_and_malformed_outputs_are_sanitized_and_never_persisted() {
    for mode in [
        "error",
        "exit",
        "provider",
        "confidence",
        "score",
        "foreign",
        "count",
        "time",
        "extra",
        "voter",
        "missing",
    ] {
        let root = Root::new();
        let observer = Observer::new();
        let mut node = root.node(observer.provider(mode));
        let (author, _) = author(&mut node);
        let claim = node.publish_text(&author.id, "Public claim").unwrap();
        let evidence = node
            .publish_text(
                &author.id,
                "According to a study, the dataset supports the claim.",
            )
            .unwrap();
        edge(&mut node, &author, &evidence, &claim, Relation::Supports);
        let all_before = node.store().list_judgments().unwrap();
        let api = Api::start(&root, node).await;
        let token = api.register().await;
        let before = history(&api, &claim.id).await;
        let failed = api
            .request(
                "POST",
                &format!("/judgments/object/{}", claim.id),
                Some(&token),
                json!({"definition":DEFINITION,"parameters":{}}),
            )
            .await;
        assert_eq!(
            failed.0,
            StatusCode::SERVICE_UNAVAILABLE,
            "{mode}: {failed:?}"
        );
        assert!(failed.1.get("judgment").is_none());
        for secret in [
            PRIVATE,
            token.as_str(),
            "traceback",
            root.0.to_str().unwrap(),
        ] {
            assert!(
                !failed.1.to_string().contains(secret),
                "{mode}: leaked provider details"
            );
        }
        assert_eq!(
            history(&api, &claim.id).await,
            before,
            "{mode}: persisted failed result"
        );
        if matches!(mode, "error" | "foreign") {
            let failed_rpc = ok(api
                .rpc(
                    "babel.judgment.object.evaluate.v1",
                    &token,
                    json!({"object_id":claim.id,"definition":DEFINITION,"parameters":{}}),
                )
                .await);
            assert!(!failed_rpc["error"].is_null(), "{mode}: {failed_rpc}");
            assert!(failed_rpc["result"].is_null());
            assert!(!failed_rpc.to_string().contains(PRIVATE));
            assert!(!failed_rpc.to_string().contains("traceback"));
        }
        let frames = observer.frames();
        let real = frames
            .iter()
            .find(|frame| frame["request"]["request"]["definition"] == DEFINITION)
            .unwrap();
        assert!(
            real["response"]["error"].is_null(),
            "{mode}: real algorithm failed before injection"
        );
        assert_eq!(real["response"]["result"]["output"]["validation_count"], 1);
        api.stop().await;
        let node = root.python();
        assert_eq!(
            node.store().list_judgments().unwrap(),
            all_before,
            "{mode}: orphaned judgment persisted"
        );
        assert_eq!(
            serde_json::to_value(node.object_judgments(&claim.id).unwrap()).unwrap(),
            json!(before)
        );
        assert!(
            node.store()
                .object_judgment_inputs(&claim.id)
                .unwrap()
                .iter()
                .all(|i| i.request.definition.as_str() != DEFINITION)
        );
    }
}

#[tokio::test]
async fn excessive_source_count_individual_bytes_and_total_bytes_fail_without_truncation() {
    for (case, count, bytes) in [
        ("count", 201, 32),
        ("individual", 1, 64 * 1024 + 1),
        ("total", 9, 60 * 1024),
    ] {
        let root = Root::new();
        let observer = Observer::new();
        let mut node = root.node(observer.provider("valid"));
        let (author, key) = author(&mut node);
        let claim = node
            .publish_text(&author.id, "Bounded public evidence")
            .unwrap();
        let mut bundle = ImportBundle::default();
        for index in 0..count {
            let prefix = format!("Source {index:03}: ");
            let text = prefix.clone() + &"x".repeat(bytes - prefix.len());
            let object = Object::text(&author, text)
                .unwrap()
                .sign(&author, &key)
                .unwrap();
            let edge = Edge::new(
                object.id.clone(),
                claim.id.clone(),
                Relation::Supports,
                EdgeOrigin::HumanAssertion,
                Some(author.id.clone()),
            )
            .unwrap()
            .sign(&author, &key)
            .unwrap();
            bundle.objects.push(object);
            bundle.edges.push(edge);
        }
        node.import_bundle(bundle).unwrap();
        let all_before = node.store().list_judgments().unwrap();
        let api = Api::start(&root, node).await;
        let token = api.register().await;
        let before = observer.frames();
        let rejected = api
            .request(
                "POST",
                &format!("/judgments/object/{}", claim.id),
                Some(&token),
                json!({"definition":DEFINITION,"parameters":{}}),
            )
            .await;
        assert_eq!(rejected.0, StatusCode::CONFLICT, "{case}: {rejected:?}");
        assert!(rejected.1.get("judgment").is_none());
        assert_eq!(
            observer.frames(),
            before,
            "{case}: limits must be enforced before provider execution"
        );
        api.stop().await;
        let node = root.python();
        assert_eq!(
            node.store().list_judgments().unwrap(),
            all_before,
            "{case}: failed evaluation persisted"
        );
    }
}

#[tokio::test]
async fn explicit_rust_local_rejects_source_agreement_without_a_synthetic_result() {
    let root = Root::new();
    let mut node = root.node(JudgmentConfig::RustLocal.start().unwrap());
    let (author, _) = author(&mut node);
    let claim = node
        .publish_text(&author.id, "No local consensus substitute")
        .unwrap();
    let api = Api::start(&root, node).await;
    let token = api.register().await;
    let before = history(&api, &claim.id).await;
    let providers = api.get("/judgments/providers").await;
    assert!(
        !providers["providers"][0]["supported_definitions"]
            .as_array()
            .unwrap()
            .contains(&json!(DEFINITION))
    );
    let failed = api
        .request(
            "POST",
            &format!("/judgments/object/{}", claim.id),
            Some(&token),
            json!({"definition":DEFINITION}),
        )
        .await;
    assert!(!failed.0.is_success(), "{failed:?}");
    assert!(failed.1.get("judgment").is_none());
    assert_eq!(history(&api, &claim.id).await, before);
    api.stop().await;
}
