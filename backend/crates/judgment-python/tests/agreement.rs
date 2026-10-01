use babble_judgment::{
    AgreementSource, AgreementSourceKind, DefinitionId, JudgmentProvider, JudgmentRegistry,
    JudgmentRequest, JudgmentState, SourceAgreementInput, SourceAgreementOutput,
    validate_source_agreement_result,
};
use babble_judgment_python::{PythonProvider, WorkerConfig, contract};
use babble_types::{Canonical, Error};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

fn input(count: usize) -> SourceAgreementInput {
    SourceAgreementInput {
        reference_time: 200.0,
        previous_score: None,
        sources: (0..count)
            .map(|i| AgreementSource {
                source_id: format!("source-{i}"),
                kind: AgreementSourceKind::ResearchPaper,
                text: "The measured result is reproducible.".into(),
                timestamp: 100.0,
                quality_score: 0.8,
                evidence_score: 0.7,
                user_id: Some(format!("user-{i}")),
                vote: Some(0.9),
                is_context: false,
            })
            .collect(),
    }
}

fn request(input: &SourceAgreementInput) -> JudgmentRequest {
    JudgmentRequest {
        definition: DefinitionId::source_agreement_v1(),
        state: JudgmentState {
            subject: "obj_test".into(),
            context: BTreeMap::from([
                (
                    "text".into(),
                    json!("Subject remains separate from sources."),
                ),
                (
                    "source_agreement".into(),
                    serde_json::to_value(input).unwrap(),
                ),
            ]),
        },
        parameters: BTreeMap::new(),
    }
}

fn config(mode: Option<&str>) -> WorkerConfig {
    WorkerConfig {
        executable: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../algorithms/.venv/bin/python"),
        args: mode.map_or_else(
            || vec!["-I".into(), "-m".into(), "babble_algorithms.worker".into()],
            |mode| vec!["-I".into(), "-c".into(), FAULT_WORKER.into(), mode.into()],
        ),
        working_directory: None,
        timeout: Duration::from_secs(5),
    }
}

#[test]
fn real_worker_empty_full_batch_health_privacy_and_durable_commitments() {
    let provider = PythonProvider::new(config(None)).unwrap();
    assert_eq!(provider.supported_definitions().len(), 7);
    assert!(
        provider
            .privacy_policy()
            .allowed_context_keys
            .contains("source_agreement")
    );
    assert!(
        !babble_judgment::JudgmentPrivacyPolicy::local_full()
            .allowed_context_keys
            .contains("source_agreement")
    );
    assert!(
        !babble_judgment::JudgmentPrivacyPolicy::remote_minimized()
            .allowed_context_keys
            .contains("source_agreement")
    );
    for count in [0, 1, 2, 200] {
        let req = request(&input(count));
        contract::encode(&contract::Request::judge(2, req.clone())).unwrap();
        let actual = provider.judge(&req).unwrap();
        assert_eq!(actual.provider, contract::provider());
        assert_eq!(actual.confidence, 0.0);
        assert_eq!(actual.input_hash, req.state.canonical_hash().unwrap());
        validate_source_agreement_result(&req, &actual.output).unwrap();
        let output: SourceAgreementOutput = serde_json::from_value(actual.output).unwrap();
        assert_eq!(output.validation_count, count);
        assert_eq!(output.user_contributions.len(), count);
        assert_eq!(
            output.source_ids,
            (0..count)
                .map(|i| format!("source-{i}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(actual.id, provider.judge(&req).unwrap().id);
    }
    let mut req = request(&input(0));
    let before = provider.judge(&req).unwrap();
    req.state.context.get_mut("source_agreement").unwrap()["previous_score"] = json!(0.9);
    let after = provider.judge(&req).unwrap();
    assert_eq!(after.output["state"], "revoked");
    assert_ne!(before.id, after.id);
}

#[test]
fn canonical_input_rejects_nonfinite_and_out_of_range_values() {
    for bad in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -62167219201.0,
        253402300800.0,
    ] {
        let mut value = input(1);
        value.reference_time = bad;
        assert!(value.validate().is_err());
        value = input(1);
        value.sources[0].timestamp = bad;
        assert!(value.validate().is_err());
    }
    for bad in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        for field in 0..4 {
            let mut value = input(1);
            match field {
                0 => value.previous_score = Some(bad),
                1 => value.sources[0].quality_score = bad,
                2 => value.sources[0].evidence_score = bad,
                _ => value.sources[0].vote = Some(bad),
            }
            assert!(value.validate().is_err());
        }
    }
    let mut value = input(1);
    value.sources[0].timestamp = 201.0;
    assert!(value.validate().is_err());
    value = input(1);
    value.sources[0].user_id = None;
    assert!(value.validate().is_err());
    value = input(1);
    value.sources.push(value.sources[0].clone());
    assert!(value.validate().is_err());
    assert!(input(201).validate().is_err());
}

#[test]
fn required_nullable_fields_unknown_fields_and_invalid_types_fail_before_worker() {
    let provider = PythonProvider::new(config(None)).unwrap();
    let valid = request(&input(1));
    let check = |req: &JudgmentRequest| {
        assert!(
            JudgmentRegistry::babble_core()
                .validate_request(req)
                .is_err()
        );
        assert!(matches!(provider.judge(req), Err(Error::Conflict(_))));
        assert!(contract::encode(&contract::Request::judge(2, req.clone())).is_err());
    };
    for field in ["reference_time", "previous_score", "sources"] {
        let mut req = valid.clone();
        req.state
            .context
            .get_mut("source_agreement")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        check(&req);
    }
    for field in [
        "source_id",
        "kind",
        "text",
        "timestamp",
        "quality_score",
        "evidence_score",
        "user_id",
        "vote",
        "is_context",
    ] {
        let mut req = valid.clone();
        req.state.context.get_mut("source_agreement").unwrap()["sources"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        check(&req);
    }
    for (pointer, bad) in [
        ("/sources/0/timestamp", json!(true)),
        ("/sources/0/quality_score", json!("0.5")),
        ("/sources/0/is_context", json!(1)),
        ("/sources/0/kind", json!("unknown")),
        ("/sources/0/source_id", json!("\u{00e9}".repeat(257))),
        ("/sources/0/user_id", json!(" ")),
        ("/previous_score", json!(false)),
    ] {
        let mut req = valid.clone();
        *req.state
            .context
            .get_mut("source_agreement")
            .unwrap()
            .pointer_mut(pointer)
            .unwrap() = bad;
        check(&req);
    }
    for pointer in ["", "/sources/0"] {
        let mut req = valid.clone();
        req.state
            .context
            .get_mut("source_agreement")
            .unwrap()
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        check(&req);
    }
    assert!(provider.judge(&valid).is_ok());
}

#[test]
fn exact_utf8_text_and_timestamp_limits_cross_real_worker() {
    let provider = PythonProvider::new(config(None)).unwrap();
    let mut value = input(8);
    value.reference_time = 253402300799.0;
    for source in &mut value.sources {
        source.text = "\u{00e9}".repeat(32768);
        source.timestamp = -62167219200.0;
        source.user_id = None;
        source.vote = None;
    }
    value.sources[0].source_id = "\u{00e9}".repeat(256);
    let req = request(&value);
    let output = provider.judge(&req).unwrap().output;
    validate_source_agreement_result(&req, &output).unwrap();
    assert_eq!(output["temporal_weight"], 0.0);
    value.sources[0].text.push('x');
    assert!(value.validate().is_err());
    value.sources[0].text.pop();
    let mut extra = input(1).sources.remove(0);
    extra.source_id = "extra".into();
    extra.text = "x".into();
    value.sources.push(extra);
    assert!(value.validate().is_err());
}

#[test]
fn generic_output_binding_rejects_changed_subject_time_order_count_voters_and_extra_fields() {
    let req = request(&input(2));
    let valid = PythonProvider::new(config(None))
        .unwrap()
        .judge(&req)
        .unwrap()
        .output;
    for (field, bad) in [
        ("content_id", json!("other")),
        ("reference_time", json!(201)),
        ("source_ids", json!(["source-1", "source-0"])),
        ("validation_count", json!(1)),
        ("user_contributions", json!({"unknown": 0.5})),
        ("confidence", json!(0.1)),
        ("confidence_status", json!("calibrated")),
        ("state", json!("true")),
        ("reliability_score", json!(1.1)),
        ("limitations", json!([])),
        ("extra", json!(true)),
    ] {
        let mut output = valid.clone();
        output[field] = bad;
        assert!(
            validate_source_agreement_result(&req, &output).is_err(),
            "{field}"
        );
    }
    for key in valid.as_object().unwrap().keys() {
        let mut output = valid.clone();
        output.as_object_mut().unwrap().remove(key);
        assert!(
            validate_source_agreement_result(&req, &output).is_err(),
            "{key}"
        );
    }
    // The boundary validates a score's domain, not the provider's formula.
    let mut valid_alternative = valid;
    valid_alternative["consensus_score"] = json!(0.123);
    validate_source_agreement_result(&req, &valid_alternative).unwrap();
}

#[test]
fn hostile_worker_outputs_fail_closed_and_restart_with_new_handshake() {
    let req = request(&input(2));
    for mode in [
        "subject",
        "time",
        "order",
        "count",
        "voters",
        "extra",
        "missing",
        "nan",
        "score",
        "confidence",
        "status",
        "provider",
        "stale",
        "error",
        "exit",
        "duplicate",
    ] {
        let provider = PythonProvider::new(config(Some(mode))).unwrap();
        let err = provider.judge(&req).expect_err(mode);
        assert!(
            matches!(err, Error::ProviderUnavailable(_)),
            "{mode}: {err}"
        );
        assert!(!err.to_string().contains("PRIVATE"));
        assert!(provider.judge(&req).is_ok(), "restart after {mode}");
    }
    let provider = PythonProvider::new(config(Some("six_health")));
    assert!(provider.is_err());
}

#[test]
fn schema_requires_nullable_fields_and_explicit_agreement_context() {
    let schema = contract::schemas();
    let defs = &schema["request"]["$defs"];
    for (name, field) in [
        ("SourceAgreementInput", "previous_score"),
        ("AgreementSource", "user_id"),
        ("AgreementSource", "vote"),
    ] {
        assert!(
            defs[name]["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field))
        );
        assert!(
            defs[name]["properties"][field]["type"]
                .as_array()
                .unwrap()
                .contains(&json!("null"))
        );
    }
    assert_eq!(defs["SourceAgreementInput"]["additionalProperties"], false);
    assert_eq!(defs["AgreementSource"]["additionalProperties"], false);
    assert_eq!(
        defs["SourceAgreementInput"]["properties"]["reference_time"]["exclusiveMaximum"],
        253402300800.0
    );
}

const FAULT_WORKER: &str = r#"
import json, sys
from dataclasses import asdict
from babble_algorithms.execution import AlgorithmExecutor
from babble_algorithms.worker import handle
executor = AlgorithmExecutor()
mode = sys.argv[1]
for line in sys.stdin.buffer:
    request = json.loads(line)
    response = asdict(handle(line, executor))
    if request['method'] == 'health' and mode == 'six_health':
        response['result']['supported_definitions'] = response['result']['supported_definitions'][:-1]
    if request['method'] == 'judge' and request['id'] == 2:
        result = response['result']
        output = result['output']
        if mode == 'subject': output['content_id'] = 'other'
        if mode == 'time': output['reference_time'] += 1
        if mode == 'order': output['source_ids'] = tuple(reversed(output['source_ids']))
        if mode == 'count': output['validation_count'] = 0
        if mode == 'voters': output['user_contributions'] = {'unknown': 0.5}
        if mode == 'extra': output['extra'] = True
        if mode == 'missing': del output['term_agreement']
        if mode == 'nan': output['term_agreement'] = float('nan')
        if mode == 'score': output['term_agreement'] = 1.1
        if mode == 'confidence': result['confidence'] = output['confidence'] = 0.5
        if mode == 'status': output['confidence_status'] = 'calibrated'
        if mode == 'provider': result['provider']['version'] = '999'
        if mode == 'stale': response['id'] -= 1
        if mode == 'exit': sys.exit(17)
        if mode == 'error':
            response['result'] = None
            response['error'] = {'code': 'algorithm_failure', 'message': 'PRIVATE traceback'}
        if mode == 'duplicate':
            print(json.dumps(response).replace('"consensus_score":', '"consensus_score": 0, "consensus_score":'), flush=True)
            continue
    print(json.dumps(response), flush=True)
"#;
