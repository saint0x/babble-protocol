use babble_judgment::{
    DefinitionId, JudgmentProvider, JudgmentRegistry, JudgmentRequest, JudgmentState,
};
use babble_judgment_python::{PythonProvider, WorkerConfig};
use babble_types::Canonical;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}
fn python() -> PathBuf {
    std::env::var_os("BABBLE_TEST_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("algorithms/.venv/bin/python"))
}
fn config(mode: &str) -> WorkerConfig {
    WorkerConfig {
        executable: python(),
        args: vec![
            "-I".into(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/worker.py")
                .display()
                .to_string(),
            mode.into(),
        ],
        working_directory: None,
        timeout: Duration::from_secs(2),
    }
}
fn request() -> JudgmentRequest {
    JudgmentRequest {
        definition: DefinitionId::spam_v1(),
        state: JudgmentState {
            subject: "obj_worker_test".into(),
            context: BTreeMap::from([(
                "text".into(),
                json!("A study with dataset evidence supports the protocol."),
            )]),
        },
        parameters: BTreeMap::new(),
    }
}
fn pid(judgment: &babble_judgment::Judgment) -> i32 {
    judgment.output["pid"].as_i64().unwrap() as i32
}
fn gone(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == -1 }
}

#[test]
fn persistent_and_drop_reaps_worker() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PythonProvider>();
    let provider = PythonProvider::new(config("good")).unwrap();
    let first = provider.judge(&request()).unwrap();
    let second = provider.judge(&request()).unwrap();
    assert_eq!(pid(&first), pid(&second));
    assert_eq!(first.id, second.id);
    assert_eq!(first.input_hash, request().state.canonical_hash().unwrap());
    drop(provider);
    assert!(
        gone(pid(&first)),
        "drop must kill and reap before returning"
    );
}

#[test]
fn rejects_faults_without_private_error_leakage() {
    for mode in [
        "truncated",
        "oversize",
        "malformed",
        "invalid_utf8",
        "stale_id",
        "exit",
        "protocol",
        "provider",
        "shape",
        "score",
        "confidence",
        "confidence_mismatch",
        "nan",
        "nested_nan",
        "judgment_nodes",
        "error",
        "duplicate",
        "missing_null",
        "both",
        "multiple",
    ] {
        let provider = PythonProvider::new(config(mode)).unwrap();
        let error = provider.judge(&request()).expect_err(mode).to_string();
        assert!(!error.contains("PRIVATE-CONTENT"), "{mode}: {error}");
        assert!(!error.contains("traceback"), "{mode}: {error}");
        assert!(error.contains("Python algorithm worker"), "{mode}: {error}");
    }
}

#[test]
fn health_is_real_and_validated() {
    for mode in [
        "health_provider",
        "health_definitions",
        "health_ranking",
        "health_timeout",
    ] {
        let mut cfg = config(mode);
        cfg.timeout = Duration::from_millis(750);
        let start = Instant::now();
        assert!(PythonProvider::new(cfg).is_err(), "{mode}");
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    let mut cfg = config("good");
    cfg.executable = "/no/such/python-worker".into();
    assert!(PythonProvider::new(cfg).is_err());
}

#[test]
fn reads_and_blocked_writes_share_deadline() {
    for mode in ["timeout", "blocked_write"] {
        let mut cfg = config(mode);
        cfg.timeout = Duration::from_millis(750);
        let provider = PythonProvider::new(cfg).unwrap();
        let mut req = request();
        req.state
            .context
            .insert("text".into(), json!("x".repeat(128 * 1024)));
        let start = Instant::now();
        assert!(
            provider
                .judge(&req)
                .unwrap_err()
                .to_string()
                .contains("deadline")
        );
        assert!(start.elapsed() < Duration::from_secs(2), "{mode}");
    }
}

#[test]
fn subsequent_request_restarts_after_failure() {
    let provider = PythonProvider::new(config("restart")).unwrap();
    assert!(provider.judge(&request()).is_err());
    assert!(provider.judge(&request()).is_ok());
}

#[test]
fn environment_is_explicit_and_parameters_are_strict() {
    let provider = PythonProvider::new(config("environment")).unwrap();
    let result = provider.judge(&request()).unwrap();
    for key in result.output["environment_keys"].as_array().unwrap() {
        assert!(
            ["PATH", "LANG", "LC_CTYPE", "__CF_USER_TEXT_ENCODING"]
                .contains(&key.as_str().unwrap()),
            "unexpected inherited key {key}"
        );
    }
    let mut req = request();
    req.parameters
        .insert("future_parameter".into(), json!(true));
    assert!(matches!(
        provider.judge(&req),
        Err(babble_types::Error::Conflict(_))
    ));
    req.definition = DefinitionId::moderation_v1();
    for parameter in [
        json!({"policy":{"spam_limit":true}}),
        json!({"context":{"reports":1.5}}),
        json!({"policy":{"future":0.5}}),
        json!({"context":null}),
    ] {
        req.parameters = serde_json::from_value(parameter).unwrap();
        assert!(matches!(
            provider.judge(&req),
            Err(babble_types::Error::Conflict(_))
        ));
    }
}

#[test]
fn input_limits_reject_before_transport() {
    let provider = PythonProvider::new(config("good")).unwrap();
    let base = request();
    let mut cases = Vec::new();
    let mut req = base.clone();
    req.state.subject = "x".repeat(4097);
    cases.push(req);
    let mut req = base.clone();
    req.state
        .context
        .insert("text".into(), json!("x".repeat(131073)));
    cases.push(req);
    let mut req = base.clone();
    req.state.context.insert("text".into(), json!(" "));
    cases.push(req);
    let mut req = base.clone();
    for i in 0..65 {
        req.parameters.insert(format!("p{i}"), json!(1));
    }
    cases.push(req);
    let mut req = base.clone();
    let mut nested = json!(1);
    for _ in 0..17 {
        nested = json!([nested]);
    }
    req.parameters.insert("nested".into(), nested);
    cases.push(req);
    let mut req = base.clone();
    for i in 0..10 {
        req.parameters
            .insert(format!("p{i}"), json!("x".repeat(131072)));
    }
    cases.push(req);
    for req in cases {
        assert!(provider.judge(&req).is_err());
    }
    assert!(provider.judge(&base).is_ok());
}

#[test]
fn real_python_seven_definitions_and_parameter_commitments() {
    // Required integration: a missing installation fails with actionable setup guidance.
    let cfg = WorkerConfig {
        executable: python(),
        args: vec!["-I".into(), "-m".into(), "babble_algorithms.worker".into()],
        working_directory: Some(root().join("algorithms")),
        timeout: Duration::from_secs(5),
    };
    let provider = PythonProvider::new(cfg).expect("real worker required: run uv sync --directory algorithms, or set BABBLE_TEST_PYTHON to its installed interpreter");
    for definition in provider.supported_definitions() {
        let mut req = request();
        req.definition = definition;
        if req.definition == DefinitionId::source_agreement_v1() {
            req.state.context.insert(
                "source_agreement".into(),
                json!({
                    "reference_time": 0.0, "previous_score": null, "sources": []
                }),
            );
        }
        if req.definition == DefinitionId::relationship_v1() {
            req.parameters.insert("relation".into(), json!("supports"));
        }
        if req.definition == DefinitionId::relevance_v1() {
            req.parameters
                .insert("query".into(), json!("dataset protocol"));
        }
        let judgment = provider.judge(&req).unwrap();
        assert_eq!(judgment.provider, provider.version());
        JudgmentRegistry::babble_core()
            .validate_output(&req.definition, &judgment.output)
            .unwrap();
        assert_eq!(judgment.input_hash, req.state.canonical_hash().unwrap());
        assert_eq!(judgment.id, provider.judge(&req).unwrap().id);
    }
    let mut req = request();
    req.definition = DefinitionId::relevance_v1();
    req.parameters.insert("query".into(), json!("dataset"));
    let first = provider.judge(&req).unwrap();
    req.parameters.insert("query".into(), json!("protocol"));
    let second = provider.judge(&req).unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(first.input_hash, second.input_hash);
    let fixtures: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root().join("fixtures/algorithms/v1/fixtures.json")).unwrap(),
    )
    .unwrap();
    for (index, wire) in fixtures["requests"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, wire)| wire["method"] == "judge")
    {
        let req: JudgmentRequest = serde_json::from_value(wire["request"].clone()).unwrap();
        let actual = provider.judge(&req).unwrap();
        let expected = &fixtures["responses"][index]["result"];
        assert_eq!(
            actual.output,
            expected["output"],
            "fixture drift for {}",
            req.definition.as_str()
        );
        assert_eq!(actual.confidence, expected["confidence"].as_f64().unwrap());
    }
}

#[test]
fn local_orchestration_preserves_public_pair_scope_and_commits_both_texts() {
    use babble_judgment::{JudgmentCache, JudgmentOrchestrator, JudgmentPrivacyPolicy};

    let provider = PythonProvider::new(WorkerConfig {
        executable: python(),
        args: vec!["-I".into(), "-m".into(), "babble_algorithms.worker".into()],
        working_directory: Some(root().join("algorithms")),
        timeout: Duration::from_secs(5),
    })
    .expect("real installed Python worker required");
    let orchestrator = JudgmentOrchestrator::single(&provider);
    let mut cache = JudgmentCache::default();
    let mut req = JudgmentRequest {
        definition: DefinitionId::relationship_v1(),
        state: JudgmentState {
            subject: "obj_public_pair".into(),
            context: BTreeMap::from([
                (
                    "text".into(),
                    json!("ordinary source\nTarget: contradicts the hypothesis"),
                ),
                ("source_text".into(), json!("ordinary source")),
                ("target_text".into(), json!("contradicts the hypothesis")),
                ("private_history".into(), json!("must not reach the worker")),
            ]),
        },
        parameters: BTreeMap::from([("relation".into(), json!("contradicts"))]),
    };
    let (scoped, _) = provider.privacy_policy().apply(&req).unwrap();
    assert_eq!(scoped.state.context["source_text"], "ordinary source");
    assert_eq!(
        scoped.state.context["target_text"],
        "contradicts the hypothesis"
    );
    assert!(!scoped.state.context.contains_key("private_history"));
    let first = orchestrator.evaluate(&mut cache, &req).unwrap().judgment;
    assert_eq!(first.output["marker_scope"], "source_text");
    assert_eq!(first.output["score"], 0.0);
    assert_eq!(first.output["target_context_evaluated"], false);
    assert_eq!(first.input_hash, scoped.state.canonical_hash().unwrap());
    assert_eq!(
        first.id,
        orchestrator.evaluate(&mut cache, &req).unwrap().judgment.id
    );

    // Keep the combined legacy field identical: the explicit pair is material input.
    req.state
        .context
        .insert("target_text".into(), json!("a different target"));
    let changed_target = orchestrator.evaluate(&mut cache, &req).unwrap().judgment;
    assert_ne!(first.input_hash, changed_target.input_hash);
    assert_ne!(first.id, changed_target.id);
    assert_eq!(changed_target.output["score"], 0.0);
    req.state
        .context
        .insert("source_text".into(), json!("contradicts the hypothesis"));
    let changed_source = orchestrator.evaluate(&mut cache, &req).unwrap().judgment;
    assert!(changed_source.output["score"].as_f64().unwrap() > 0.0);
    assert_ne!(changed_source.id, changed_target.id);

    // This narrowly scoped local repair must not expand the remote disclosure policy.
    let (remote, _) = JudgmentPrivacyPolicy::remote_minimized()
        .apply(&req)
        .unwrap();
    assert!(!remote.state.context.contains_key("source_text"));
    assert!(!remote.state.context.contains_key("target_text"));
    assert!(!remote.state.context.contains_key("private_history"));
}
