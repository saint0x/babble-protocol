use babel_judgment::{
    DefinitionId, Judgment, JudgmentCache, JudgmentOrchestrator, JudgmentProvider, JudgmentRequest,
    JudgmentState, cache_key,
};
use babel_judgment_python::{PythonProvider, WorkerConfig};
use babel_types::{Error, Result};
use serde_json::json;
use std::{
    collections::BTreeMap,
    os::unix::net::UnixDatagram,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

fn config(timeout: Duration, signal: Option<&Signal>, mode: &str) -> WorkerConfig {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    WorkerConfig {
        executable: std::env::var_os("BABEL_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("algorithms/.venv/bin/python")),
        args: vec![
            "-I".into(),
            "-u".into(),
            "-c".into(),
            WORKER.into(),
            signal
                .map(|signal| signal.path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            mode.into(),
        ],
        working_directory: None,
        timeout,
    }
}

fn request(seconds: f64) -> JudgmentRequest {
    JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: format!("delay:{seconds}"),
            context: BTreeMap::from([(
                "text".into(),
                json!("A published dataset and methodology support the claim."),
            )]),
        },
        parameters: BTreeMap::new(),
    }
}

fn deadline_error<T: std::fmt::Debug>(result: Result<T>) {
    let error = result.unwrap_err();
    assert!(matches!(error, Error::ProviderUnavailable(_)), "{error}");
    assert!(error.to_string().contains("deadline"), "{error}");
}

fn pid(judgment: &Judgment) -> i32 {
    judgment.output["pid"].as_i64().unwrap() as i32
}

struct Signal {
    socket: UnixDatagram,
    path: PathBuf,
}

impl Signal {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = PathBuf::from(format!(
            "/tmp/babel-budget-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let socket = UnixDatagram::bind(&path).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        Self { socket, path }
    }

    fn received(&self) {
        let mut bytes = [0; 32];
        assert!(self.socket.recv(&mut bytes).unwrap() > 0);
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn supporting_assessments_and_aggregate_share_one_deadline() {
    let provider = PythonProvider::new(config(Duration::from_secs(3), None, "normal")).unwrap();
    let orchestrator = JudgmentOrchestrator::single(&provider);
    let mut cache = JudgmentCache::default();
    let started = Instant::now();
    let deadline = started + Duration::from_millis(700);
    let first = request(0.1);
    let mut second = request(0.1);
    second.state.context.insert(
        "text".into(),
        json!("Independent published dataset evidence."),
    );
    orchestrator
        .evaluate_before(&mut cache, &first, deadline)
        .unwrap();
    orchestrator
        .evaluate_before(&mut cache, &second, deadline)
        .unwrap();
    let mut aggregate = request(0.7);
    aggregate.definition = DefinitionId::source_agreement_v1();
    aggregate.state.context.insert(
        "source_agreement".into(),
        json!({"reference_time":0.0,"previous_score":null,"sources":[]}),
    );
    deadline_error(orchestrator.evaluate_before(&mut cache, &aggregate, deadline));
    assert!(started.elapsed() < Duration::from_millis(1400));
    assert!(
        cache
            .entry(&cache_key(&provider.version(), &aggregate).unwrap())
            .is_none()
    );
    // Completed evaluations remain committed; only the failed evaluation rolls back.
    for completed in [&first, &second] {
        assert!(
            cache
                .entry(&cache_key(&provider.version(), completed).unwrap())
                .is_some()
        );
    }
    deadline_error(orchestrator.evaluate_before(&mut cache, &first, deadline));
    assert_eq!(
        cache
            .entry(&cache_key(&provider.version(), &first).unwrap())
            .unwrap()
            .hits,
        0
    );
    assert!(provider.judge(&request(0.0)).is_ok());
}

#[test]
fn contention_and_expired_requests_do_not_disturb_in_flight_worker() {
    let signal = Signal::new();
    let provider =
        PythonProvider::new(config(Duration::from_secs(3), Some(&signal), "normal")).unwrap();
    thread::scope(|scope| {
        let active = scope.spawn(|| provider.judge(&request(0.5)));
        signal.received();
        let started = Instant::now();
        deadline_error(provider.judge_before(&request(0.0), started));
        assert!(started.elapsed() < Duration::from_millis(100));
        let started = Instant::now();
        deadline_error(provider.judge_before(&request(0.0), started + Duration::from_millis(100)));
        assert!(started.elapsed() < Duration::from_millis(350));
        let first = active.join().unwrap().unwrap();
        let next = provider.judge(&request(0.0)).unwrap();
        assert_eq!(pid(&first), pid(&next));
        assert_eq!(
            next.output["calls"], 2,
            "expired queued calls must never reach Python"
        );
    });
}

#[test]
fn lock_wait_and_exchange_consume_the_same_budget() {
    let signal = Signal::new();
    let provider =
        PythonProvider::new(config(Duration::from_secs(3), Some(&signal), "normal")).unwrap();
    thread::scope(|scope| {
        let active = scope.spawn(|| provider.judge(&request(0.25)));
        signal.received();
        let started = Instant::now();
        deadline_error(provider.judge_before(&request(0.4), started + Duration::from_millis(450)));
        let first = active.join().unwrap().unwrap();
        assert!(started.elapsed() < Duration::from_millis(850));
        assert_eq!(
            unsafe { libc::kill(pid(&first), 0) },
            -1,
            "interrupted worker must be reaped"
        );
        let restarted = provider.judge(&request(0.0)).unwrap();
        assert_ne!(pid(&first), pid(&restarted));
        assert_eq!(restarted.output["calls"], 1);
    });
}

#[test]
fn configured_timeout_is_a_ceiling_for_external_deadlines_and_plain_judge() {
    let provider = PythonProvider::new(config(Duration::from_millis(750), None, "normal")).unwrap();
    for external in [false, true] {
        let started = Instant::now();
        let result = if external {
            provider.judge_before(&request(2.0), started + Duration::from_secs(5))
        } else {
            provider.judge(&request(2.0))
        };
        deadline_error(result);
        assert!(started.elapsed() < Duration::from_millis(1600));
        assert!(provider.judge(&request(0.0)).is_ok());
    }
}

#[test]
fn expired_after_timeout_does_not_spawn_or_consume_worker_ids() {
    let provider = PythonProvider::new(config(Duration::from_secs(3), None, "normal")).unwrap();
    deadline_error(
        provider.judge_before(&request(2.0), Instant::now() + Duration::from_millis(100)),
    );
    for _ in 0..10 {
        deadline_error(provider.judge_before(&request(0.0), Instant::now()));
    }
    let next = provider.judge(&request(0.0)).unwrap();
    assert_eq!(
        next.output["health_id"], 3,
        "expired calls must not start a replacement"
    );
    assert_eq!(next.output["request_id"], 4);
    assert_eq!(next.output["calls"], 1);
}

#[test]
fn restart_health_handshake_obeys_external_budget() {
    let provider =
        PythonProvider::new(config(Duration::from_secs(3), None, "slow-restart")).unwrap();
    deadline_error(
        provider.judge_before(&request(2.0), Instant::now() + Duration::from_millis(100)),
    );
    let started = Instant::now();
    deadline_error(provider.judge_before(&request(0.0), started + Duration::from_millis(150)));
    assert!(started.elapsed() < Duration::from_millis(600));
}

#[test]
fn blocked_writes_use_the_external_deadline() {
    let provider =
        PythonProvider::new(config(Duration::from_secs(3), None, "blocked-write")).unwrap();
    let mut req = request(0.0);
    req.state
        .context
        .insert("text".into(), json!("x".repeat(128 * 1024)));
    let started = Instant::now();
    deadline_error(provider.judge_before(&req, started + Duration::from_millis(150)));
    assert!(started.elapsed() < Duration::from_millis(600));
}

// The real executor supplies health and all judgment outputs. Only scheduling
// and observability are added here; no substitute judgment implementation.
const WORKER: &str = r#"
import json, os, socket, sys, time
from dataclasses import asdict
from babel_algorithms.execution import AlgorithmExecutor
from babel_algorithms.worker import handle
executor = AlgorithmExecutor()
signal, mode = sys.argv[1:]
calls = 0
health_id = None
for line in sys.stdin.buffer:
    request = json.loads(line)
    if request['method'] == 'health':
        health_id = request['id']
        if mode == 'slow-restart' and health_id > 1:
            time.sleep(30)
    if request['method'] == 'judge':
        calls += 1
        if signal:
            with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as sock:
                sock.sendto(str(request['id']).encode(), signal)
        time.sleep(float(request['request']['state']['subject'].split(':')[1]))
    response = asdict(handle(line, executor))
    if request['method'] == 'judge' and request['request']['definition'] != 'babel.judgment.source_agreement.v1':
        response['result']['output'].update(pid=os.getpid(), calls=calls, health_id=health_id, request_id=request['id'])
    print(json.dumps(response), flush=True)
    if request['method'] == 'health' and mode == 'blocked-write':
        time.sleep(30)
"#;
