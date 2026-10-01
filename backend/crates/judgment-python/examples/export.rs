use babble_discovery::{TemporalProvider, TemporalRequest};
use babble_judgment::{JudgmentProvider, JudgmentRequest, JudgmentState};
use babble_judgment_python::{
    PythonProvider, WorkerConfig,
    contract::{
        self, ErrorCode, HealthResult, JudgeResult, Protocol, Request, Response, WorkerError,
        WorkerResult,
    },
};
use babble_lens::{RankingProvider, RankingRequest};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let directory = root.join("fixtures/algorithms/v1");
    let provider = PythonProvider::new(WorkerConfig {
        executable: std::env::var_os("BABBLE_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("algorithms/.venv/bin/python")),
        args: vec!["-I".into(), "-m".into(), "babble_algorithms.worker".into()],
        working_directory: None,
        timeout: Duration::from_secs(5),
    })?;
    let mut requests = vec![Request::health(1)];
    let mut responses = vec![Response {
        protocol: Protocol::V1,
        id: Some(1),
        result: Some(WorkerResult::Health(HealthResult {
            provider: JudgmentProvider::version(&provider),
            supported_definitions: contract::definitions(),
            ranking_provider: contract::ranking_provider(),
            temporal_provider: contract::temporal_provider(),
        })),
        error: None,
    }];
    for (index, definition) in contract::definitions().into_iter().enumerate() {
        let mut request = JudgmentRequest {
            definition,
            state: JudgmentState {
                subject: "obj_worker_fixture".into(),
                context: BTreeMap::from([(
                    "text".into(),
                    json!(
                        "According to a study, the dataset supports this useful protocol. However, replication contradicts the original claim."
                    ),
                )]),
            },
            parameters: BTreeMap::new(),
        };
        match request.definition.as_str() {
            "babble.judgment.source_agreement.v1" => {
                request.state.context.insert(
                    "source_agreement".into(),
                    json!({
                        "reference_time": 200.0,
                        "previous_score": null,
                        "sources": [
                            {"source_id": "fixture-research", "kind": "research_paper",
                             "text": "The dataset is reproducible.", "timestamp": 100.0,
                             "quality_score": 0.8, "evidence_score": 0.7,
                             "user_id": "fixture-user", "vote": 0.9, "is_context": false},
                            {"source_id": "fixture-context", "kind": "context",
                             "text": "The dataset is reproducible.", "timestamp": 150.0,
                             "quality_score": 0.6, "evidence_score": 0.5,
                             "user_id": null, "vote": null, "is_context": true}
                        ]
                    }),
                );
            }
            "babble.judgment.relevance.v1" => {
                request
                    .parameters
                    .insert("query".into(), json!("dataset protocol"));
            }
            "babble.judgment.relationship.v1" => {
                request
                    .parameters
                    .insert("relation".into(), json!("supports"));
            }
            "babble.judgment.moderation.v1" => {
                request
                    .parameters
                    .insert("context".into(), json!({"reports": 2}));
                request
                    .parameters
                    .insert("policy".into(), json!({"quality_warn":0.4}));
            }
            _ => {}
        }
        let judgment = provider.judge(&request)?;
        let id = index as u64 + 2;
        requests.push(Request::judge(id, request));
        responses.push(Response {
            protocol: Protocol::V1,
            id: Some(id),
            result: Some(WorkerResult::Judge(JudgeResult {
                provider: judgment.provider,
                output: judgment.output,
                confidence: judgment.confidence,
            })),
            error: None,
        });
    }
    let golden: serde_json::Value = serde_json::from_slice(&std::fs::read(
        root.join("fixtures/protocol/v1/ranking.json"),
    )?)?;
    for case in golden["cases"]
        .as_array()
        .ok_or("missing ranking fixtures")?
    {
        let request: RankingRequest = serde_json::from_value(case["request"].clone())?;
        let result = provider.rank(&request)?;
        let id = requests.len() as u64 + 1;
        requests.push(Request::rank(id, request));
        responses.push(Response {
            protocol: Protocol::V1,
            id: Some(id),
            result: Some(WorkerResult::Rank(result)),
            error: None,
        });
    }
    let temporal: serde_json::Value = serde_json::from_slice(&std::fs::read(
        root.join("fixtures/protocol/v1/fixtures.json"),
    )?)?;
    for case in temporal["temporal_scoring"]["cases"]
        .as_array()
        .ok_or("missing temporal fixtures")?
    {
        let request: TemporalRequest = serde_json::from_value(case["request"].clone())?;
        let result = provider.score(&request)?;
        let id = requests.len() as u64 + 1;
        requests.push(Request::temporal(id, request));
        responses.push(Response {
            protocol: Protocol::V1,
            id: Some(id),
            result: Some(WorkerResult::Temporal(result)),
            error: None,
        });
    }
    responses.push(Response {
        protocol: Protocol::V1,
        id: None,
        result: None,
        error: Some(WorkerError {
            code: ErrorCode::InvalidRequest,
            message: "Invalid algorithm worker request.".into(),
        }),
    });
    std::fs::create_dir_all(&directory)?;
    for (name, value) in [
        ("schema-bundle.json", contract::schemas()),
        (
            "fixtures.json",
            json!({"protocol": Protocol::V1, "requests": requests, "responses": responses}),
        ),
    ] {
        std::fs::write(
            directory.join(name),
            format!("{}\n", serde_json::to_string_pretty(&value)?),
        )?;
    }
    Ok(())
}
