use babble_discovery::{CandidateEngine, DiscoveryRequest, ObjectSignals};
use babble_eval::{
    DiscoveryMixEvalCase, JudgmentAgreement, JudgmentEvalCase, JudgmentExpectation, LensEvalCase,
    evaluate_discovery_mix, evaluate_judgment_corpus, evaluate_judgment_provider_matrix,
    evaluate_lens_corpus,
};
use babble_graph::{Edge, EdgeOrigin, GraphIndex, Relation};
use babble_judgment::{DefinitionId, JudgmentRequest, JudgmentState};
use babble_judgment_jev::{JevConfig, JevProvider, JevRequest, JevResponse, JevTransport};
use babble_judgment_local::LocalProvider;
use babble_lens::{
    BuiltInLens, Candidate, CandidateSource, CandidateSourceContribution, EvidenceSignals,
    LensStack, LensWeight, ReputationSignals, Signals,
};
use babble_types::{ObjectId, Timestamp};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn local_judgment_provider_passes_versioned_calibration_corpus() {
    let report = evaluate_judgment_corpus(&LocalProvider::default(), &judgment_corpus()).unwrap();

    report.assert_passed().unwrap();
    assert_eq!(report.provider, "babble-local");
    assert_eq!(report.cases, 8);
    assert_eq!(report.failures, Vec::new());
}

#[test]
fn local_and_jev_compatible_providers_pass_matrix_calibration() {
    let local = LocalProvider::default();
    let jev = JevProvider::with_transport(
        JevConfig::new(
            "https://jev.invalid/evaluate",
            "jev-compatible-eval",
            "fixture-1",
        ),
        DeterministicJevTransport,
    );
    let providers: [&dyn babble_judgment::JudgmentProvider; 2] = [&local, &jev];
    let report = evaluate_judgment_provider_matrix(&providers, &provider_matrix_corpus()).unwrap();

    report.assert_passed().unwrap();
    assert_eq!(report.cases, 7);
    assert_eq!(report.providers.len(), 2);
    assert_eq!(report.provider_outputs, 14);
}

#[test]
fn built_in_lenses_pass_frozen_ranking_corpus() {
    let report = evaluate_lens_corpus(&lens_corpus());

    report.assert_passed().unwrap();
    assert_eq!(report.cases, 4);
    assert!(report.average_distinct_sources >= 4.0);
}

#[test]
fn candidate_engine_passes_discovery_source_mix_corpus() {
    let report = evaluate_discovery_mix(&discovery_mix_corpus());

    report.assert_passed().unwrap();
    assert_eq!(report.cases, 1);
    assert!(report.average_candidates >= 5.0);
    assert!(report.average_distinct_primary_sources >= 4.0);
}

fn judgment_corpus() -> Vec<JudgmentEvalCase> {
    vec![
        judgment_case(
            "spam-heavy-promotion",
            DefinitionId::spam_v1(),
            "BUY NOW!!! Limited time free money, click here https://example.invalid",
            BTreeMap::new(),
            vec![JudgmentExpectation::ScoreAtLeast {
                field: "score".to_string(),
                value: 0.75,
            }],
        ),
        judgment_case(
            "spam-clean-research-note",
            DefinitionId::spam_v1(),
            "According to the dataset, the reproducible protocol measurement improved.",
            BTreeMap::new(),
            vec![JudgmentExpectation::ScoreAtMost {
                field: "score".to_string(),
                value: 0.05,
            }],
        ),
        judgment_case(
            "evidence-rich-methodology",
            DefinitionId::evidence_quality_v1(),
            "According to the DOI dataset and reproduced methodology, the source confirms the measurement.",
            BTreeMap::new(),
            vec![JudgmentExpectation::ScoreAtLeast {
                field: "score".to_string(),
                value: 0.60,
            }],
        ),
        judgment_case(
            "relevance-query-match",
            DefinitionId::relevance_v1(),
            "Babble runtime capability sandboxes isolate executable Object Surfaces.",
            BTreeMap::from([("query".to_string(), json!("runtime capability"))]),
            vec![JudgmentExpectation::ScoreAtLeast {
                field: "score".to_string(),
                value: 1.0,
            }],
        ),
        judgment_case(
            "relationship-supports",
            DefinitionId::relationship_v1(),
            "The dataset evidence supports the claim because repeated runs confirm it.",
            BTreeMap::from([("relation".to_string(), json!("supports"))]),
            vec![JudgmentExpectation::ScoreAtLeast {
                field: "score".to_string(),
                value: 0.70,
            }],
        ),
        judgment_case(
            "content-analysis-runtime-topic",
            DefinitionId::content_analysis_v1(),
            "The runtime Surface uses a capability sandbox and WebAssembly resource budget.",
            BTreeMap::new(),
            vec![JudgmentExpectation::IncludesString {
                field: "topics".to_string(),
                value: "runtime".to_string(),
            }],
        ),
        judgment_case(
            "moderation-network-integrity",
            DefinitionId::moderation_v1(),
            "This phishing exploit will steal credentials with malware and weaponized links.",
            BTreeMap::new(),
            vec![
                JudgmentExpectation::Action {
                    value: "remove".to_string(),
                },
                JudgmentExpectation::IncludesString {
                    field: "flags".to_string(),
                    value: "safety".to_string(),
                },
            ],
        ),
        judgment_case(
            "moderation-evidence-allow",
            DefinitionId::moderation_v1(),
            "According to the dataset and reproduced methodology, the protocol report is useful.",
            BTreeMap::new(),
            vec![JudgmentExpectation::Action {
                value: "allow".to_string(),
            }],
        ),
    ]
}

fn provider_matrix_corpus() -> Vec<babble_eval::JudgmentProviderMatrixCase> {
    judgment_corpus()
        .into_iter()
        .filter(|case| {
            matches!(
                case.request.definition.as_str(),
                "babble.judgment.spam.v1"
                    | "babble.judgment.evidence_quality.v1"
                    | "babble.judgment.relevance.v1"
                    | "babble.judgment.relationship.v1"
                    | "babble.judgment.moderation.v1"
            )
        })
        .map(|case| {
            let agreements = if case.request.definition == DefinitionId::moderation_v1() {
                vec![JudgmentAgreement::StringFieldEqual {
                    field: "action".to_string(),
                }]
            } else {
                vec![JudgmentAgreement::ScoreDeltaAtMost {
                    field: "score".to_string(),
                    value: 0.20,
                }]
            };
            babble_eval::JudgmentProviderMatrixCase {
                name: case.name,
                request: case.request,
                expectations: case.expectations,
                agreements,
            }
        })
        .collect()
}

fn judgment_case(
    name: &str,
    definition: DefinitionId,
    text: &str,
    parameters: BTreeMap<String, serde_json::Value>,
    expectations: Vec<JudgmentExpectation>,
) -> JudgmentEvalCase {
    JudgmentEvalCase {
        name: name.to_string(),
        request: JudgmentRequest {
            definition,
            state: JudgmentState {
                subject: name.to_string(),
                context: BTreeMap::from([("text".to_string(), json!(text))]),
            },
            parameters,
        },
        expectations,
    }
}

fn lens_corpus() -> Vec<LensEvalCase> {
    let candidates = frozen_candidates();
    vec![
        LensEvalCase {
            name: "research-prioritizes-evidence".to_string(),
            stack: LensStack::new(
                "eval-research",
                vec![LensWeight {
                    lens: BuiltInLens::Research,
                    weight: 1.0,
                }],
            ),
            candidates: candidates.clone(),
            expected_top: ObjectId::new_unchecked("obj_research_evidence"),
            required_sources: vec![CandidateSource::Evidence, CandidateSource::Contradiction],
            min_distinct_sources: 4,
        },
        LensEvalCase {
            name: "contradictions-prioritize-counterevidence".to_string(),
            stack: LensStack::new(
                "eval-contradictions",
                vec![LensWeight {
                    lens: BuiltInLens::Contradictions,
                    weight: 1.0,
                }],
            ),
            candidates: candidates.clone(),
            expected_top: ObjectId::new_unchecked("obj_counterevidence"),
            required_sources: vec![CandidateSource::Contradiction],
            min_distinct_sources: 4,
        },
        LensEvalCase {
            name: "weird-prioritizes-novel-exploration".to_string(),
            stack: LensStack::new(
                "eval-weird",
                vec![LensWeight {
                    lens: BuiltInLens::Weird,
                    weight: 1.0,
                }],
            ),
            candidates: candidates.clone(),
            expected_top: ObjectId::new_unchecked("obj_strange_tool"),
            required_sources: vec![CandidateSource::Exploration, CandidateSource::Emerging],
            min_distinct_sources: 4,
        },
        LensEvalCase {
            name: "emerging-prioritizes-new-creators".to_string(),
            stack: LensStack::new(
                "eval-emerging",
                vec![LensWeight {
                    lens: BuiltInLens::Emerging,
                    weight: 1.0,
                }],
            ),
            candidates,
            expected_top: ObjectId::new_unchecked("obj_emerging_creator"),
            required_sources: vec![CandidateSource::Emerging],
            min_distinct_sources: 4,
        },
    ]
}

fn discovery_mix_corpus() -> Vec<DiscoveryMixEvalCase> {
    let claim = object_id("obj_discovery_claim");
    let support = object_id("obj_discovery_support");
    let counter = object_id("obj_discovery_counter");
    let adjacent = object_id("obj_discovery_adjacent");
    let emerging = object_id("obj_discovery_emerging");
    let odd = object_id("obj_discovery_odd");

    let mut graph = GraphIndex::default();
    graph.insert(edge(&support, &claim, Relation::EvidenceFor));
    graph.insert(edge(&counter, &claim, Relation::EvidenceAgainst));
    graph.insert(edge(&claim, &adjacent, Relation::References));

    let summaries = BTreeMap::from([
        (
            claim.clone(),
            discovery_signals(&claim, true, 0.9, 0.25, 0.4, 0.1, 0.6, 0.72, 0.05),
        ),
        (
            support.clone(),
            discovery_signals(&support, false, 0.82, 0.2, 1.0, 0.05, 0.88, 0.64, 0.08),
        ),
        (
            counter.clone(),
            discovery_signals(&counter, false, 0.72, 0.35, 0.72, 1.0, 0.72, 0.5, 0.18),
        ),
        (
            adjacent.clone(),
            discovery_signals(&adjacent, false, 0.58, 0.62, 0.45, 0.12, 0.52, 0.4, 0.25),
        ),
        (
            emerging.clone(),
            discovery_signals(&emerging, false, 0.38, 0.88, 0.42, 0.05, 0.44, 1.0, 0.78),
        ),
        (
            odd.clone(),
            discovery_signals(&odd, false, 0.08, 1.0, 0.24, 0.02, 0.32, 0.45, 1.0),
        ),
    ]);
    let candidates = CandidateEngine.generate(
        &graph,
        &summaries,
        &DiscoveryRequest {
            anchors: vec![claim],
            followed_objects: BTreeSet::from([support.clone()]),
            limit: 10,
            exploration_slots: 1,
        },
    );

    vec![DiscoveryMixEvalCase {
        name: "graph-semantic-emerging-exploration-mix".to_string(),
        candidates,
        required_primary_sources: vec![
            CandidateSource::Following,
            CandidateSource::Contradiction,
            CandidateSource::SemanticNeighborhood,
            CandidateSource::Emerging,
        ],
        required_contributed_sources: vec![
            CandidateSource::Following,
            CandidateSource::Temporal,
            CandidateSource::Evidence,
            CandidateSource::Contradiction,
            CandidateSource::SemanticNeighborhood,
            CandidateSource::Emerging,
            CandidateSource::Exploration,
        ],
        min_candidates: 6,
        min_distinct_primary_sources: 4,
    }]
}

fn frozen_candidates() -> Vec<Candidate> {
    vec![
        candidate(
            "obj_research_evidence",
            CandidateSource::Evidence,
            Signals {
                followed_author: true,
                relevance: 0.92,
                novelty: 0.42,
                evidence_quality: 0.98,
                contradiction: 0.16,
                temporal: 0.36,
                exploration: 0.10,
                reputation: ReputationSignals {
                    epistemic_accuracy: 0.94,
                    evidence_quality: 0.96,
                    domain_expertise: 0.92,
                    social_constructiveness: 0.74,
                    creative_contribution: 0.42,
                    moderation: 0.88,
                },
                evidence: EvidenceSignals {
                    human_support: 1.0,
                    judgment_support: 0.9,
                    human_contradiction: 0.0,
                    judgment_contradiction: 0.0,
                },
                ..base_signals()
            },
        ),
        candidate(
            "obj_counterevidence",
            CandidateSource::Contradiction,
            Signals {
                relevance: 0.72,
                novelty: 0.55,
                evidence_quality: 0.55,
                contradiction: 1.0,
                temporal: 0.50,
                exploration: 0.24,
                reputation: ReputationSignals {
                    epistemic_accuracy: 0.80,
                    evidence_quality: 0.82,
                    domain_expertise: 0.70,
                    social_constructiveness: 0.65,
                    creative_contribution: 0.35,
                    moderation: 0.80,
                },
                evidence: EvidenceSignals {
                    human_support: 0.0,
                    judgment_support: 0.0,
                    human_contradiction: 1.0,
                    judgment_contradiction: 0.95,
                },
                ..base_signals()
            },
        ),
        candidate(
            "obj_strange_tool",
            CandidateSource::Exploration,
            Signals {
                relevance: 0.12,
                novelty: 1.0,
                evidence_quality: 0.30,
                contradiction: 0.10,
                temporal: 0.62,
                exploration: 1.0,
                reputation: ReputationSignals {
                    creative_contribution: 0.96,
                    social_constructiveness: 0.55,
                    ..ReputationSignals::default()
                },
                ..base_signals()
            },
        ),
        candidate(
            "obj_emerging_creator",
            CandidateSource::Emerging,
            Signals {
                social_distance: 0.82,
                relevance: 0.35,
                novelty: 0.86,
                evidence_quality: 0.42,
                contradiction: 0.06,
                temporal: 1.0,
                exploration: 0.82,
                reputation: ReputationSignals {
                    creative_contribution: 0.88,
                    social_constructiveness: 0.48,
                    ..ReputationSignals::default()
                },
                ..base_signals()
            },
        ),
    ]
}

fn candidate(id: &str, source: CandidateSource, signals: Signals) -> Candidate {
    Candidate {
        object_id: ObjectId::new_unchecked(id),
        source: source.clone(),
        sources: vec![CandidateSourceContribution {
            source,
            weight: 1.0,
        }],
        created_at: Timestamp::now(),
        signals: signals.bounded(),
    }
}

#[allow(clippy::too_many_arguments)]
fn discovery_signals(
    object_id: &ObjectId,
    followed_author: bool,
    relevance: f64,
    novelty: f64,
    evidence_quality: f64,
    contradiction: f64,
    reputation: f64,
    temporal: f64,
    exploration: f64,
) -> ObjectSignals {
    ObjectSignals {
        object_id: object_id.clone(),
        created_at: Timestamp::now(),
        followed_author,
        relevance,
        novelty,
        evidence_quality,
        contradiction,
        evidence: EvidenceSignals {
            human_support: evidence_quality * 3.0,
            judgment_support: evidence_quality,
            human_contradiction: contradiction * 3.0,
            judgment_contradiction: contradiction,
        },
        reputation: ReputationSignals {
            epistemic_accuracy: reputation,
            evidence_quality: reputation,
            social_constructiveness: reputation,
            creative_contribution: reputation,
            moderation: reputation,
            domain_expertise: reputation,
        },
        temporal,
        exploration,
    }
}

fn edge(source: &ObjectId, target: &ObjectId, relation: Relation) -> Edge {
    Edge::new(
        source.clone(),
        target.clone(),
        relation,
        EdgeOrigin::ApplicationAssertion,
        None,
    )
    .unwrap()
}

fn object_id(value: &str) -> ObjectId {
    ObjectId::new_unchecked(value)
}

fn base_signals() -> Signals {
    Signals {
        social_distance: 0.5,
        followed_author: false,
        relevance: 0.0,
        novelty: 0.0,
        evidence_quality: 0.0,
        contradiction: 0.0,
        evidence: EvidenceSignals::default(),
        reputation: ReputationSignals::default(),
        temporal: 0.0,
        exploration: 0.0,
    }
}

#[derive(Clone, Debug)]
struct DeterministicJevTransport;

impl JevTransport for DeterministicJevTransport {
    fn evaluate(
        &self,
        _config: &JevConfig,
        request: &JevRequest,
    ) -> babble_types::Result<JevResponse> {
        let mirrored_request = JudgmentRequest {
            definition: DefinitionId::new(request.definition.clone()),
            state: request.state.clone(),
            parameters: request.parameters.clone().into_iter().collect(),
        };
        let judgment =
            babble_judgment::JudgmentProvider::judge(&LocalProvider::default(), &mirrored_request)?;
        Ok(JevResponse {
            output: judgment.output,
            confidence: judgment.confidence,
            model: Some("jev-compatible-eval".to_string()),
            model_version: Some("fixture-1".to_string()),
        })
    }
}
