use babel_discovery::{
    NativeTemporalScorer, TemporalClass, TemporalEngagement, TemporalItem, TemporalProvider,
    TemporalRequest, TemporalResult,
};
use babel_judgment::{DefinitionId, JudgmentProvider, JudgmentRequest, JudgmentState};
use babel_judgment_python::{PythonProvider, WorkerConfig, contract};
use babel_types::{ObjectId, Timestamp};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

fn timestamp(value: &str) -> Timestamp {
    serde_json::from_value(json!(value)).unwrap()
}

fn request() -> TemporalRequest {
    TemporalRequest {
        reference_time: timestamp("2026-09-30T12:00:00Z"),
        items: vec![TemporalItem {
            object_id: ObjectId::new_unchecked(format!("obj_{:064x}", 1)),
            published_at: timestamp("2026-09-29T12:00:00Z"),
            content_class: TemporalClass::Discussion,
            quality_score: 0.5,
            tags: vec![],
            engagement: TemporalEngagement {
                total_views: 100,
                recent_views: 25,
                total_interactions: 20,
                recent_interactions: 10,
            },
        }],
    }
}

fn config(mode: Option<&str>) -> WorkerConfig {
    WorkerConfig {
        executable: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../algorithms/.venv/bin/python"),
        args: mode.map_or_else(
            || vec!["-I".into(), "-m".into(), "babel_algorithms.worker".into()],
            |mode| vec!["-I".into(), "-c".into(), FAULT_WORKER.into(), mode.into()],
        ),
        working_directory: None,
        timeout: Duration::from_secs(3),
    }
}

fn close(a: f64, b: f64) {
    assert!(
        (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0),
        "{a} != {b}"
    );
}

fn parity(provider: &PythonProvider, request: &TemporalRequest) {
    let expected = NativeTemporalScorer.score(request).unwrap();
    let actual = provider.score(request).unwrap();
    actual
        .validate_for(request, &contract::temporal_provider())
        .unwrap();
    for (a, b) in actual.scores.iter().zip(expected.scores) {
        assert_eq!(a.object_id, b.object_id);
        for (a, b) in [
            (a.age_hours, b.age_hours),
            (a.recency, b.recency),
            (a.decay_rate, b.decay_rate),
            (a.time_sensitivity, b.time_sensitivity),
            (a.engagement_velocity, b.engagement_velocity),
            (a.survival_score, b.survival_score),
        ] {
            close(a, b);
        }
    }
}

#[test]
fn temporal_native_math_covers_recency_boundaries_classes_tags_and_extremes() {
    let mut req = request();
    req.items[0].engagement = TemporalEngagement::default();
    for (hours, base) in [
        (0, 1.0_f64),
        (2, 1.0),
        (24, 0.82),
        (72, 0.62),
        (168, 0.42),
        (720, 0.23),
        (721, 0.1),
    ] {
        for (class, weight, sensitivity) in [
            (TemporalClass::News, 1.2, 0.92),
            (TemporalClass::Discussion, 1.0, 0.68),
            (TemporalClass::Analysis, 0.82, 0.5),
            (TemporalClass::Tutorial, 0.66, 0.28),
            (TemporalClass::Reference, 0.45, 0.12),
        ] {
            req.items[0].published_at =
                Timestamp(req.reference_time.0 - time::Duration::hours(hours));
            req.items[0].content_class = class;
            let result = NativeTemporalScorer.score(&req).unwrap();
            let s = &result.scores[0];
            close(s.age_hours, hours as f64);
            close(s.recency, (base * weight).min(1.0));
            close(s.time_sensitivity, sensitivity);
            close(
                s.decay_rate,
                (0.1 + 0.16 * sensitivity - 0.06).clamp(0.01, 0.5),
            );
            close(
                s.survival_score,
                s.recency * (-s.decay_rate * hours as f64 / 24.0).exp(),
            );
        }
    }
    req.items[0].tags = vec![
        "TIME-SEN\u{017f}ITIVE".into(),
        "BREAKING".into(),
        "evergreen".into(),
    ];
    close(
        NativeTemporalScorer.score(&req).unwrap().scores[0].time_sensitivity,
        0.12,
    );
    req.items[0].tags = vec!["brea\u{212a}ing".into()];
    close(
        NativeTemporalScorer.score(&req).unwrap().scores[0].time_sensitivity,
        0.3,
    );
    req.items[0].published_at = Timestamp(req.reference_time.0 + time::Duration::hours(1));
    let future = NativeTemporalScorer.score(&req).unwrap();
    assert_eq!(future.scores[0].age_hours, 0.0);
    assert_eq!(future.scores[0].engagement_velocity, 0.0);
    req.reference_time = timestamp("9999-12-31T23:59:59.999999999-23:59");
    req.items[0].published_at = timestamp("0000-01-01T00:00:00+23:59");
    let oldest = NativeTemporalScorer.score(&req).unwrap();
    assert!(oldest.scores[0].age_hours > 87_000_000.0);
    assert_eq!(oldest.scores[0].survival_score, 0.0);
}

#[test]
fn temporal_requests_reject_noncanonical_and_unbounded_inputs() {
    let valid = request();
    let reject = |req: TemporalRequest| {
        assert!(req.validate().is_err());
        assert!(NativeTemporalScorer.score(&req).is_err());
        assert!(contract::encode(&contract::Request::temporal(1, req)).is_err());
    };
    for quality in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        let mut req = valid.clone();
        req.items[0].quality_score = quality;
        reject(req);
    }
    for id in [
        "x".into(),
        format!("obj_{}", "F".repeat(64)),
        format!("obj_{}", "g".repeat(64)),
    ] {
        let mut req = valid.clone();
        req.items[0].object_id = ObjectId::new_unchecked(id);
        reject(req);
    }
    let mut req = valid.clone();
    req.items.push(req.items[0].clone());
    reject(req);
    let mut req = valid.clone();
    req.items = vec![req.items[0].clone(); 201];
    reject(req);
    for tags in [
        vec!["x".into(); 33],
        vec!["x".repeat(65)],
        vec!["\u{00e9}".repeat(33)],
    ] {
        let mut req = valid.clone();
        req.items[0].tags = tags;
        reject(req);
    }
    for e in [
        TemporalEngagement {
            total_views: 9_007_199_254_740_992,
            ..Default::default()
        },
        TemporalEngagement {
            recent_views: 1,
            ..Default::default()
        },
        TemporalEngagement {
            recent_interactions: 1,
            ..Default::default()
        },
        TemporalEngagement {
            total_interactions: 9_007_199_254_740_992,
            ..Default::default()
        },
    ] {
        let mut req = valid.clone();
        req.items[0].engagement = e;
        reject(req);
    }
    let mut req = valid.clone();
    req.reference_time = Timestamp(
        req.reference_time
            .0
            .to_offset(time::UtcOffset::from_hms(0, 0, 1).unwrap()),
    );
    reject(req);
    let mut req = valid;
    req.items[0].published_at = Timestamp(
        time::Date::from_calendar_date(-1, time::Month::January, 1)
            .unwrap()
            .midnight()
            .assume_utc(),
    );
    reject(req);
}

#[test]
fn temporal_result_validation_checks_scope_and_bounds_without_rescoring() {
    let req = request();
    let version = NativeTemporalScorer.version();
    let good = NativeTemporalScorer.score(&req).unwrap();
    for field in [
        "age_hours",
        "recency",
        "decay_rate",
        "time_sensitivity",
        "engagement_velocity",
        "survival_score",
    ] {
        for bad in [-1.0, 99_999_999.0] {
            let mut value = serde_json::to_value(&good).unwrap();
            value["scores"][0][field] = json!(bad);
            let result: TemporalResult = serde_json::from_value(value).unwrap();
            assert!(result.validate_for(&req, &version).is_err(), "{field}");
        }
    }
    let mut result = good.clone();
    result.scores[0].age_hours += 0.1;
    assert!(result.validate_for(&req, &version).is_err());
    let mut result = good.clone();
    result.scores[0].survival_score = f64::NAN;
    assert!(result.validate_for(&req, &version).is_err());
    let mut result = good.clone();
    result.reference_time = Timestamp(
        result
            .reference_time
            .0
            .to_offset(time::UtcOffset::from_hms(1, 0, 0).unwrap()),
    );
    assert!(result.validate_for(&req, &version).is_err());
    let mut result = good;
    result.scores[0].survival_score = 0.12345;
    result.validate_for(&req, &version).unwrap();
}

#[test]
fn temporal_real_python_matches_native_at_nanosecond_boundaries_and_full_batches() {
    let provider = PythonProvider::new(config(None)).unwrap();
    let mut req = request();
    let sample = req.items[0].clone();
    req.items = (0..200)
        .map(|i| {
            let mut item = sample.clone();
            item.object_id = ObjectId::new_unchecked(format!("obj_{i:064x}"));
            item.content_class = [
                TemporalClass::News,
                TemporalClass::Discussion,
                TemporalClass::Analysis,
                TemporalClass::Tutorial,
                TemporalClass::Reference,
            ][i % 5];
            item.published_at = Timestamp(
                req.reference_time.0
                    - time::Duration::hours([0, 2, 24, 72, 168, 720, 721][i % 7])
                    - time::Duration::nanoseconds((i % 3) as i64 - 1),
            );
            item.tags = vec!["\u{00e9}".repeat(32); 32];
            item.tags[i % 32] = "TIME-SEN\u{017f}ITIVE".into();
            item.tags[(i + 1) % 32] = "brea\u{212a}ing".into();
            item.quality_score = (i % 11) as f64 / 10.0;
            item.engagement = TemporalEngagement {
                total_views: 9_007_199_254_740_991,
                recent_views: i as u64,
                total_interactions: 9_007_199_254_740_991,
                recent_interactions: 9_007_199_254_740_990,
            };
            item
        })
        .collect();
    parity(&provider, &req);
    req.items.truncate(1);
    req.reference_time = timestamp("9999-12-31T23:59:59.999999999-23:59");
    req.items[0].published_at = timestamp("0000-01-01T00:00:00+23:59");
    parity(&provider, &req);
    req.reference_time = timestamp("0000-01-01T00:00:00.000000001Z");
    req.items[0].published_at = timestamp("0000-01-01T00:00:00Z");
    parity(&provider, &req);
    req.items.clear();
    parity(&provider, &req);
}

#[test]
fn temporal_hostile_worker_results_fail_closed_and_restart() {
    let mut req = request();
    let mut second = req.items[0].clone();
    second.object_id = ObjectId::new_unchecked(format!("obj_{:064x}", 2));
    req.items.push(second);
    for mode in [
        "provider",
        "extra",
        "nested_extra",
        "duplicate",
        "missing",
        "foreign",
        "order",
        "time",
        "age",
        "score",
        "decay",
        "nan",
        "stale",
        "error",
        "exit",
        "duplicate_key",
        "wrong_variant",
    ] {
        let provider = PythonProvider::new(config(Some(mode))).unwrap();
        let error = provider.score(&req).expect_err(mode).to_string();
        assert!(!error.contains("PRIVATE-CONTENT"), "{mode}: {error}");
        assert!(error.contains("Python algorithm worker"), "{mode}: {error}");
        // Fault only affects ID 2; success requires a fresh health handshake.
        assert!(provider.score(&req).is_ok(), "restart after {mode}");
    }
    for mode in ["health_missing", "health_changed", "health_extra"] {
        assert!(PythonProvider::new(config(Some(mode))).is_err(), "{mode}");
    }
}

#[test]
fn temporal_deadline_and_shared_judgment_worker() {
    let mut cfg = config(Some("timeout"));
    cfg.timeout = Duration::from_millis(750);
    let provider = PythonProvider::new(cfg).unwrap();
    let start = Instant::now();
    assert!(provider.score(&request()).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(provider.score(&request()).is_ok());
    let provider = PythonProvider::new(config(Some("shared"))).unwrap();
    let judgment = JudgmentRequest {
        definition: DefinitionId::spam_v1(),
        state: JudgmentState {
            subject: "public-post".into(),
            context: BTreeMap::from([("text".into(), json!("A useful study with evidence."))]),
        },
        parameters: BTreeMap::new(),
    };
    provider.score(&request()).unwrap();
    let first = provider.judge(&judgment).unwrap();
    provider.score(&request()).unwrap();
    let second = provider.judge(&judgment).unwrap();
    assert_eq!(first.output["temporal_calls"], 1);
    assert_eq!(second.output["temporal_calls"], 2);
    assert_eq!(first.output["pid"], second.output["pid"]);
}

#[test]
fn temporal_wire_schemas_are_strict_and_judgment_bounds_remain_small() {
    let schemas = contract::schemas();
    for name in ["TemporalRequest", "TemporalItem", "TemporalEngagement"] {
        assert_eq!(
            schemas["request"]["$defs"][name]["additionalProperties"],
            false
        );
    }
    for name in ["TemporalResult", "TemporalScore", "TemporalProviderVersion"] {
        assert_eq!(
            schemas["response"]["$defs"][name]["additionalProperties"],
            false
        );
    }
    let mut value = serde_json::to_value(request()).unwrap();
    value["items"][0]["extra"] = json!(true);
    assert!(serde_json::from_value::<TemporalRequest>(value).is_err());
    let result = NativeTemporalScorer.score(&request()).unwrap();
    let mut response =
        json!({"protocol":"babel.algorithms.v1","id":1,"result":result,"error":null});
    response["result"]["scores"][0]["extra"] = json!(true);
    assert!(contract::decode(&serde_json::to_vec(&response).unwrap()).is_err());
    let response = json!({"protocol":"babel.algorithms.v1","id":1,"result":{
        "provider":contract::provider(),"confidence":0.5,
        "output":{"extra":vec![vec![0; 256]; 17]}},"error":null});
    assert!(contract::decode(&serde_json::to_vec(&response).unwrap()).is_err());
    assert_eq!(
        schemas["response"]["$defs"]["TemporalProviderVersion"]["properties"]["model"]["const"],
        "temporal-v1"
    );
    assert_eq!(
        schemas["request"]["$defs"]["TemporalRequest"]["properties"]["reference_time"]["format"],
        "date-time"
    );
}

#[test]
fn temporal_wire_rejects_non_rfc3339_date_time_separators() {
    for separator in [" ", "_", "\n", "\0"] {
        let text = format!("2026-09-30{separator}12:00:00Z");
        let mut value = serde_json::to_value(request()).unwrap();
        value["reference_time"] = json!(text);
        assert!(serde_json::from_value::<TemporalRequest>(value).is_err());
        let mut value = serde_json::to_value(request()).unwrap();
        value["items"][0]["published_at"] = json!(text);
        assert!(serde_json::from_value::<TemporalRequest>(value).is_err());
        let mut value =
            serde_json::to_value(NativeTemporalScorer.score(&request()).unwrap()).unwrap();
        value["reference_time"] = json!(text);
        assert!(serde_json::from_value::<TemporalResult>(value).is_err());
    }
}

const FAULT_WORKER: &str = r#"
import json, os, sys, time
from dataclasses import asdict
from babel_algorithms.execution import AlgorithmExecutor
from babel_algorithms.worker import handle
mode = sys.argv[1]
executor = AlgorithmExecutor()
calls = 0
for line in sys.stdin.buffer:
    request = json.loads(line)
    response = asdict(handle(line, executor))
    result = response['result']
    if request['method'] == 'health':
        if mode == 'health_missing': del result['temporal_provider']
        if mode == 'health_changed': result['temporal_provider']['version'] = '999'
        if mode == 'health_extra': result['temporal_provider']['extra'] = True
    if request['method'] == 'temporal':
        calls += 1
        result['scores'] = list(result['scores'])
        if request['id'] == 2:
            if mode == 'timeout': time.sleep(30)
            if mode == 'exit': sys.exit(17)
            if mode == 'provider': result['provider']['version'] = '999'
            if mode == 'extra': result['extra'] = True
            if mode == 'nested_extra': result['scores'][0]['extra'] = True
            if mode == 'duplicate': result['scores'][1] = result['scores'][0]
            if mode == 'missing': result['scores'].pop()
            if mode == 'foreign': result['scores'][0]['object_id'] = 'obj_' + 'f' * 64
            if mode == 'order': result['scores'] = tuple(reversed(result['scores']))
            if mode == 'time': result['reference_time'] = '2026-09-29T00:00:00Z'
            if mode == 'age': result['scores'][0]['age_hours'] += 1
            if mode == 'score': result['scores'][0]['survival_score'] = 1.01
            if mode == 'decay': result['scores'][0]['decay_rate'] = 0.001
            if mode == 'nan': result['scores'][0]['recency'] = float('nan')
            if mode == 'stale': response['id'] -= 1
            if mode == 'error':
                response['result'] = None
                response['error'] = {'code':'algorithm_failure','message':'PRIVATE-CONTENT traceback'}
            if mode == 'wrong_variant': response['result'] = asdict(executor.health())
            if mode == 'duplicate_key':
                print(json.dumps(response).replace('"age_hours":', '"age_hours": 0, "age_hours":'), flush=True)
                continue
    if request['method'] == 'judge' and mode == 'shared':
        result['output']['temporal_calls'] = calls
        result['output']['pid'] = os.getpid()
    print(json.dumps(response), flush=True)
"#;
