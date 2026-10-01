use babble_judgment::{DefinitionId, JudgmentProvider, JudgmentRequest, JudgmentState};
use babble_judgment_python::{PythonProvider, WorkerConfig};
use babble_lens::{BuiltInLens, LensWeight, RankingProvider, RankingRequest, RankingResult};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}
fn config(mode: Option<&str>) -> WorkerConfig {
    WorkerConfig {
        executable: root().join("algorithms/.venv/bin/python"),
        args: mode.map_or_else(
            || vec!["-I".into(), "-m".into(), "babble_algorithms.worker".into()],
            |mode| {
                vec![
                    "-I".into(),
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/ranking_worker.py")
                        .display()
                        .to_string(),
                    mode.into(),
                ]
            },
        ),
        working_directory: None,
        timeout: Duration::from_secs(3),
    }
}
fn cases() -> Vec<Value> {
    let fixture: Value = serde_json::from_slice(
        &std::fs::read(root().join("fixtures/protocol/v1/ranking.json")).unwrap(),
    )
    .unwrap();
    fixture["cases"].as_array().unwrap().clone()
}
fn request() -> RankingRequest {
    cases()
        .iter()
        .find_map(|case| {
            let request: RankingRequest = serde_json::from_value(case["request"].clone()).unwrap();
            (!request.candidates.is_empty()).then_some(request)
        })
        .unwrap()
}

fn equivalent(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            let a = a.as_f64().unwrap();
            let b = b.as_f64().unwrap();
            assert!(
                (a - b).abs() <= 1e-10 * a.abs().max(b.abs()).max(1.0),
                "numeric drift at {path}: {a} != {b}"
            );
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "keys at {path}");
            for (key, value) in b {
                equivalent(&a[key], value, &format!("{path}/{key}"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "array at {path}");
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                equivalent(a, b, &format!("{path}/{index}"));
            }
        }
        _ => assert_eq!(actual, expected, "drift at {path}"),
    }
}

#[test]
fn real_python_ranking_matches_canonical_native_fixtures_and_traces() {
    let provider = PythonProvider::new(config(None)).unwrap();
    let cases = cases();
    assert!(cases.len() >= 8);
    for case in cases {
        let request: RankingRequest = serde_json::from_value(case["request"].clone()).unwrap();
        let mut expected: RankingResult = serde_json::from_value(case["result"].clone()).unwrap();
        let actual = provider
            .rank(&request)
            .unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
        expected.provider = RankingProvider::version(&provider);
        equivalent(
            &serde_json::to_value(actual).unwrap(),
            &serde_json::to_value(expected).unwrap(),
            case["name"].as_str().unwrap(),
        );
    }
}

#[test]
fn full_candidate_batch_with_all_lenses_fits_the_bounded_worker_transport() {
    let provider = PythonProvider::new(config(None)).unwrap();
    let mut request = request();
    let sample = request.candidates[0].clone();
    request.candidates = (0..200)
        .map(|index| {
            let mut candidate = sample.clone();
            candidate.object_id = babble_types::ObjectId::new_unchecked(format!("obj_{index:064x}"));
            candidate
        })
        .collect();
    request.limit = 200;
    request.lens.weights = [
        BuiltInLens::Following,
        BuiltInLens::Friends,
        BuiltInLens::Research,
        BuiltInLens::Weird,
        BuiltInLens::IntellectualSerendipity,
        BuiltInLens::Contradictions,
        BuiltInLens::Emerging,
        BuiltInLens::SlowInternet,
    ]
    .into_iter()
    .map(|lens| LensWeight { lens, weight: 1.0 })
    .collect();
    let result = provider.rank(&request).unwrap();
    assert_eq!(result.ranked.len(), 200);
    assert_eq!(result.trace.candidates.len(), 200);
    assert!(
        result
            .trace
            .candidates
            .iter()
            .all(|trace| trace.lens_contributions.len() == 8)
    );
}

#[test]
fn in_memory_derived_float_signals_round_trip_without_changing_candidate_identity() {
    let provider = PythonProvider::new(config(None)).unwrap();
    let mut request = request();
    request.candidates[0].signals.relevance = 0.23025850929940458;
    request.candidates[0].signals.novelty = 0.12345678901234567;
    request.limit = request.candidates.len();
    let result = provider.rank(&request).unwrap();
    let returned = result
        .ranked
        .iter()
        .find(|entry| entry.candidate.object_id == request.candidates[0].object_id)
        .unwrap();
    assert_eq!(returned.candidate, request.candidates[0]);
}

#[test]
fn untrusted_ranking_results_fail_closed_without_diagnostic_leaks() {
    for mode in [
        "provider",
        "extra",
        "nested_extra",
        "duplicate",
        "mutation",
        "foreign",
        "trace",
        "score",
        "nan",
        "filtered",
        "stale",
        "error",
        "exit",
    ] {
        let provider = PythonProvider::new(config(Some(mode))).unwrap();
        let error = provider.rank(&request()).expect_err(mode).to_string();
        assert!(error.contains("Python algorithm worker"), "{mode}: {error}");
        assert!(!error.contains("PRIVATE-CONTENT"), "{mode}: {error}");
        assert!(!error.contains("traceback"), "{mode}: {error}");
    }
}

#[test]
fn ranking_deadlines_and_process_restart_use_existing_bounded_transport() {
    let mut timeout = config(Some("timeout"));
    timeout.timeout = Duration::from_millis(750);
    let provider = PythonProvider::new(timeout).unwrap();
    let start = Instant::now();
    assert!(provider.rank(&request()).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    let provider = PythonProvider::new(config(Some("restart"))).unwrap();
    assert!(provider.rank(&request()).is_err());
    assert!(provider.rank(&request()).is_ok());
}

#[test]
fn ranking_and_judgment_share_one_persistent_worker_without_private_inputs() {
    let provider = PythonProvider::new(config(Some("shared"))).unwrap();
    let judgment = JudgmentRequest {
        definition: DefinitionId::spam_v1(),
        state: JudgmentState {
            subject: "public-post".into(),
            context: BTreeMap::from([("text".into(), json!("A useful study with evidence."))]),
        },
        parameters: BTreeMap::new(),
    };
    provider.rank(&request()).unwrap();
    let first = provider.judge(&judgment).unwrap();
    provider.rank(&request()).unwrap();
    let second = provider.judge(&judgment).unwrap();
    assert_eq!(first.output["rank_calls"], 1);
    assert_eq!(second.output["rank_calls"], 2);
    assert_eq!(first.output["pid"], second.output["pid"]);
}
