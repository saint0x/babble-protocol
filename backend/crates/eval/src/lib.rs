use babble_hashgraph::FinalityReport;
use babble_judgment::{JudgmentProvider, JudgmentRegistry, JudgmentRequest, ProviderVersion};
use babble_lens::{Candidate, CandidateSource, LensStack};
use babble_realtime::RoomView;
use babble_types::{Error, ObjectId, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentEvalCase {
    pub name: String,
    pub request: JudgmentRequest,
    pub expectations: Vec<JudgmentExpectation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum JudgmentExpectation {
    ScoreAtLeast { field: String, value: f64 },
    ScoreAtMost { field: String, value: f64 },
    Action { value: String },
    IncludesString { field: String, value: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum JudgmentAgreement {
    ScoreDeltaAtMost { field: String, value: f64 },
    StringFieldEqual { field: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentEvalReport {
    pub provider: String,
    pub model: String,
    pub version: String,
    pub cases: usize,
    pub passed: usize,
    pub failures: Vec<EvalFailure>,
}

impl JudgmentEvalReport {
    pub fn assert_passed(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "Judgment evaluation failed {} of {} cases: {}",
                self.failures.len(),
                self.cases,
                failure_names(&self.failures)
            )))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvalFailure {
    pub case: String,
    pub expectation: String,
    pub observed: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentProviderMatrixCase {
    pub name: String,
    pub request: JudgmentRequest,
    pub expectations: Vec<JudgmentExpectation>,
    pub agreements: Vec<JudgmentAgreement>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentProviderMatrixReport {
    pub providers: Vec<String>,
    pub cases: usize,
    pub passed: usize,
    pub provider_outputs: usize,
    pub failures: Vec<EvalFailure>,
}

impl JudgmentProviderMatrixReport {
    pub fn assert_passed(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "Judgment provider matrix failed {} checks across {} cases and {} providers: {}",
                self.failures.len(),
                self.cases,
                self.providers.len(),
                failure_names(&self.failures)
            )))
        }
    }
}

pub fn evaluate_judgment_corpus<P>(
    provider: &P,
    cases: &[JudgmentEvalCase],
) -> Result<JudgmentEvalReport>
where
    P: JudgmentProvider,
{
    let registry = JudgmentRegistry::babble_core();
    let version = provider.version();
    let mut failures = Vec::new();
    let mut passed = 0usize;

    for case in cases {
        registry.validate_request(&case.request)?;
        let judgment = provider.judge(&case.request)?;
        registry.validate_output(&judgment.definition, &judgment.output)?;
        let failures_before = failures.len();
        for expectation in &case.expectations {
            if let Err(failure) = expectation.evaluate(&case.name, &judgment.output) {
                failures.push(failure);
            }
        }
        if failures.len() == failures_before {
            passed += 1;
        }
    }

    Ok(JudgmentEvalReport {
        provider: version.provider,
        model: version.model,
        version: version.version,
        cases: cases.len(),
        passed,
        failures,
    })
}

pub fn evaluate_judgment_provider_matrix(
    providers: &[&dyn JudgmentProvider],
    cases: &[JudgmentProviderMatrixCase],
) -> Result<JudgmentProviderMatrixReport> {
    let registry = JudgmentRegistry::babble_core();
    let provider_names = providers
        .iter()
        .map(|provider| provider_name(&provider.version()))
        .collect::<Vec<_>>();
    let mut failures = Vec::new();
    let mut passed = 0usize;
    let mut provider_outputs = 0usize;

    if providers.is_empty() {
        failures.push(EvalFailure {
            case: "provider_matrix".to_string(),
            expectation: "at least one provider".to_string(),
            observed: serde_json::json!(0),
        });
    }

    for case in cases {
        registry.validate_request(&case.request)?;
        let failures_before = failures.len();
        let mut outputs = Vec::new();

        for provider in providers {
            let version = provider.version();
            let output_case = format!("{}::{}", case.name, provider_name(&version));
            let judgment = provider.judge(&case.request)?;
            registry.validate_output(&judgment.definition, &judgment.output)?;
            provider_outputs += 1;
            for expectation in &case.expectations {
                if let Err(mut failure) = expectation.evaluate(&output_case, &judgment.output) {
                    failure.observed = serde_json::json!({
                        "provider": version,
                        "output": judgment.output,
                    });
                    failures.push(failure);
                }
            }
            outputs.push((version, judgment.output));
        }

        for agreement in &case.agreements {
            agreement.evaluate(&case.name, &outputs, &mut failures);
        }

        if failures.len() == failures_before {
            passed += 1;
        }
    }

    Ok(JudgmentProviderMatrixReport {
        providers: provider_names,
        cases: cases.len(),
        passed,
        provider_outputs,
        failures,
    })
}

impl JudgmentExpectation {
    fn evaluate(&self, case: &str, output: &Value) -> std::result::Result<(), EvalFailure> {
        match self {
            Self::ScoreAtLeast { field, value } => {
                let observed = output.get(field).and_then(Value::as_f64);
                if observed.is_some_and(|score| score >= *value) {
                    Ok(())
                } else {
                    Err(failure(case, format!("{field} >= {value}"), output))
                }
            }
            Self::ScoreAtMost { field, value } => {
                let observed = output.get(field).and_then(Value::as_f64);
                if observed.is_some_and(|score| score <= *value) {
                    Ok(())
                } else {
                    Err(failure(case, format!("{field} <= {value}"), output))
                }
            }
            Self::Action { value } => {
                let observed = output.get("action").and_then(Value::as_str);
                if observed == Some(value.as_str()) {
                    Ok(())
                } else {
                    Err(failure(case, format!("action == {value}"), output))
                }
            }
            Self::IncludesString { field, value } => {
                let observed = output.get(field).and_then(Value::as_array);
                let contains = observed
                    .map(|values| values.iter().any(|entry| entry.as_str() == Some(value)))
                    .unwrap_or(false);
                if contains {
                    Ok(())
                } else {
                    Err(failure(case, format!("{field} includes {value}"), output))
                }
            }
        }
    }
}

impl JudgmentAgreement {
    fn evaluate(
        &self,
        case: &str,
        outputs: &[(ProviderVersion, Value)],
        failures: &mut Vec<EvalFailure>,
    ) {
        match self {
            Self::ScoreDeltaAtMost { field, value } => {
                let scores = outputs
                    .iter()
                    .filter_map(|(provider, output)| {
                        output
                            .get(field)
                            .and_then(Value::as_f64)
                            .map(|score| (provider_name(provider), score))
                    })
                    .collect::<Vec<_>>();
                if scores.len() != outputs.len() {
                    failures.push(EvalFailure {
                        case: case.to_string(),
                        expectation: format!("{field} score present for every provider"),
                        observed: serde_json::json!(scores),
                    });
                    return;
                }
                for left in 0..scores.len() {
                    for right in (left + 1)..scores.len() {
                        let delta = (scores[left].1 - scores[right].1).abs();
                        if delta > *value {
                            failures.push(EvalFailure {
                                case: case.to_string(),
                                expectation: format!(
                                    "{field} score delta <= {value} between {} and {}",
                                    scores[left].0, scores[right].0
                                ),
                                observed: serde_json::json!(delta),
                            });
                        }
                    }
                }
            }
            Self::StringFieldEqual { field } => {
                let values = outputs
                    .iter()
                    .filter_map(|(provider, output)| {
                        output
                            .get(field)
                            .and_then(Value::as_str)
                            .map(|value| (provider_name(provider), value.to_string()))
                    })
                    .collect::<Vec<_>>();
                if values.len() != outputs.len() {
                    failures.push(EvalFailure {
                        case: case.to_string(),
                        expectation: format!("{field} string present for every provider"),
                        observed: serde_json::json!(values),
                    });
                    return;
                }
                if values.windows(2).any(|window| window[0].1 != window[1].1) {
                    failures.push(EvalFailure {
                        case: case.to_string(),
                        expectation: format!("{field} string equal across providers"),
                        observed: serde_json::json!(values),
                    });
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LensEvalCase {
    pub name: String,
    pub stack: LensStack,
    pub candidates: Vec<Candidate>,
    pub expected_top: ObjectId,
    pub required_sources: Vec<CandidateSource>,
    pub min_distinct_sources: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LensEvalReport {
    pub cases: usize,
    pub passed: usize,
    pub average_distinct_sources: f64,
    pub failures: Vec<EvalFailure>,
}

impl LensEvalReport {
    pub fn assert_passed(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "Lens evaluation failed {} of {} cases: {}",
                self.failures.len(),
                self.cases,
                failure_names(&self.failures)
            )))
        }
    }
}

pub fn evaluate_lens_corpus(cases: &[LensEvalCase]) -> LensEvalReport {
    let mut failures = Vec::new();
    let mut distinct_source_total = 0usize;
    let mut passed = 0usize;

    for case in cases {
        let failures_before = failures.len();
        let (ranked, trace) = case.stack.rank_with_trace(&case.candidates);
        let top = ranked
            .first()
            .map(|ranked| ranked.candidate.object_id.clone());
        if top.as_ref() != Some(&case.expected_top) {
            failures.push(EvalFailure {
                case: case.name.clone(),
                expectation: format!("top Object == {}", case.expected_top),
                observed: serde_json::json!(top.map(|id| id.to_string())),
            });
        }

        let observed_sources = trace
            .candidates
            .iter()
            .map(|candidate| candidate.source.clone())
            .collect::<BTreeSet<_>>();
        distinct_source_total += observed_sources.len();
        if observed_sources.len() < case.min_distinct_sources {
            failures.push(EvalFailure {
                case: case.name.clone(),
                expectation: format!("at least {} distinct sources", case.min_distinct_sources),
                observed: serde_json::json!(observed_sources.len()),
            });
        }
        for source in &case.required_sources {
            if !observed_sources.contains(source) {
                failures.push(EvalFailure {
                    case: case.name.clone(),
                    expectation: format!("source {:?} present in ranking trace", source),
                    observed: serde_json::json!(observed_sources),
                });
            }
        }
        if failures.len() == failures_before {
            passed += 1;
        }
    }

    LensEvalReport {
        cases: cases.len(),
        passed,
        average_distinct_sources: if cases.is_empty() {
            0.0
        } else {
            distinct_source_total as f64 / cases.len() as f64
        },
        failures,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveryMixEvalCase {
    pub name: String,
    pub candidates: Vec<Candidate>,
    pub required_primary_sources: Vec<CandidateSource>,
    pub required_contributed_sources: Vec<CandidateSource>,
    pub min_candidates: usize,
    pub min_distinct_primary_sources: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveryMixEvalReport {
    pub cases: usize,
    pub passed: usize,
    pub average_candidates: f64,
    pub average_distinct_primary_sources: f64,
    pub failures: Vec<EvalFailure>,
}

impl DiscoveryMixEvalReport {
    pub fn assert_passed(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "Discovery mix evaluation failed {} of {} cases: {}",
                self.failures.len(),
                self.cases,
                failure_names(&self.failures)
            )))
        }
    }
}

pub fn evaluate_discovery_mix(cases: &[DiscoveryMixEvalCase]) -> DiscoveryMixEvalReport {
    let mut failures = Vec::new();
    let mut passed = 0usize;
    let mut candidate_total = 0usize;
    let mut primary_source_total = 0usize;

    for case in cases {
        let failures_before = failures.len();
        candidate_total += case.candidates.len();
        if case.candidates.len() < case.min_candidates {
            failures.push(EvalFailure {
                case: case.name.clone(),
                expectation: format!("at least {} candidates", case.min_candidates),
                observed: serde_json::json!(case.candidates.len()),
            });
        }

        let primary_sources = case
            .candidates
            .iter()
            .map(|candidate| candidate.source.clone())
            .collect::<BTreeSet<_>>();
        primary_source_total += primary_sources.len();
        if primary_sources.len() < case.min_distinct_primary_sources {
            failures.push(EvalFailure {
                case: case.name.clone(),
                expectation: format!(
                    "at least {} distinct primary sources",
                    case.min_distinct_primary_sources
                ),
                observed: serde_json::json!(primary_sources.len()),
            });
        }
        for source in &case.required_primary_sources {
            if !primary_sources.contains(source) {
                failures.push(EvalFailure {
                    case: case.name.clone(),
                    expectation: format!("primary source {:?} present", source),
                    observed: serde_json::json!(primary_sources),
                });
            }
        }

        let contributed_sources = case
            .candidates
            .iter()
            .flat_map(|candidate| candidate.sources.iter())
            .map(|source| source.source.clone())
            .collect::<BTreeSet<_>>();
        for source in &case.required_contributed_sources {
            if !contributed_sources.contains(source) {
                failures.push(EvalFailure {
                    case: case.name.clone(),
                    expectation: format!("contributed source {:?} present", source),
                    observed: serde_json::json!(contributed_sources),
                });
            }
        }

        if failures.len() == failures_before {
            passed += 1;
        }
    }

    DiscoveryMixEvalReport {
        cases: cases.len(),
        passed,
        average_candidates: average(candidate_total, cases.len()),
        average_distinct_primary_sources: average(primary_source_total, cases.len()),
        failures,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConsensusFinalityEvalCase {
    pub name: String,
    pub report: FinalityReport,
    pub expected_validator_weight: u64,
    pub expected_supermajority_weight: u64,
    pub min_rounds: usize,
    pub min_famous_witnesses: usize,
    pub min_finalized: usize,
    pub max_undecided_witnesses: usize,
    pub require_strict_finalized_order: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConsensusFinalityEvalReport {
    pub cases: usize,
    pub passed: usize,
    pub average_finalized: f64,
    pub average_famous_witnesses: f64,
    pub failures: Vec<EvalFailure>,
}

impl ConsensusFinalityEvalReport {
    pub fn assert_passed(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "Consensus finality evaluation failed {} of {} cases: {}",
                self.failures.len(),
                self.cases,
                failure_names(&self.failures)
            )))
        }
    }
}

pub fn evaluate_consensus_finality(
    cases: &[ConsensusFinalityEvalCase],
) -> ConsensusFinalityEvalReport {
    let mut failures = Vec::new();
    let mut passed = 0usize;
    let mut finalized_total = 0usize;
    let mut famous_total = 0usize;

    for case in cases {
        let failures_before = failures.len();
        let report = &case.report;
        finalized_total += report.finalized.len();
        famous_total += report.famous_witnesses.len();

        check_eq(
            &mut failures,
            &case.name,
            "validator weight",
            report.validator_weight,
            case.expected_validator_weight,
        );
        check_eq(
            &mut failures,
            &case.name,
            "supermajority weight",
            report.supermajority_weight,
            case.expected_supermajority_weight,
        );
        check_min(
            &mut failures,
            &case.name,
            "round count",
            report.rounds.len(),
            case.min_rounds,
        );
        check_min(
            &mut failures,
            &case.name,
            "famous witness count",
            report.famous_witnesses.len(),
            case.min_famous_witnesses,
        );
        check_min(
            &mut failures,
            &case.name,
            "finalized event count",
            report.finalized.len(),
            case.min_finalized,
        );
        check_max(
            &mut failures,
            &case.name,
            "undecided witness count",
            report.undecided_witnesses.len(),
            case.max_undecided_witnesses,
        );

        if case.require_strict_finalized_order
            && !report
                .finalized
                .windows(2)
                .all(|window| window[0].index < window[1].index)
        {
            failures.push(EvalFailure {
                case: case.name.clone(),
                expectation: "strictly increasing finalized indexes".to_string(),
                observed: serde_json::json!(
                    report
                        .finalized
                        .iter()
                        .map(|event| event.index)
                        .collect::<Vec<_>>()
                ),
            });
        }

        if failures.len() == failures_before {
            passed += 1;
        }
    }

    ConsensusFinalityEvalReport {
        cases: cases.len(),
        passed,
        average_finalized: average(finalized_total, cases.len()),
        average_famous_witnesses: average(famous_total, cases.len()),
        failures,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConsensusLoadEvalCase {
    pub name: String,
    pub report: FinalityReport,
    pub validator_count: usize,
    pub inserted_events: usize,
    pub elapsed_ms: u64,
    pub min_rounds: usize,
    pub min_famous_witnesses: usize,
    pub min_finalized: usize,
    pub max_undecided_witnesses: usize,
    pub max_elapsed_ms: u64,
    pub require_strict_finalized_order: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConsensusLoadEvalReport {
    pub cases: usize,
    pub passed: usize,
    pub total_events: usize,
    pub average_events: f64,
    pub average_finalized: f64,
    pub max_elapsed_ms: u64,
    pub failures: Vec<EvalFailure>,
}

impl ConsensusLoadEvalReport {
    pub fn assert_passed(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "Consensus load evaluation failed {} of {} cases: {}",
                self.failures.len(),
                self.cases,
                failure_names(&self.failures)
            )))
        }
    }
}

pub fn evaluate_consensus_load(cases: &[ConsensusLoadEvalCase]) -> ConsensusLoadEvalReport {
    let mut failures = Vec::new();
    let mut passed = 0usize;
    let mut event_total = 0usize;
    let mut finalized_total = 0usize;
    let mut max_elapsed_ms = 0u64;

    for case in cases {
        let failures_before = failures.len();
        let report = &case.report;
        event_total += case.inserted_events;
        finalized_total += report.finalized.len();
        max_elapsed_ms = max_elapsed_ms.max(case.elapsed_ms);

        check_min(
            &mut failures,
            &case.name,
            "validator count",
            case.validator_count,
            4,
        );
        check_min(
            &mut failures,
            &case.name,
            "inserted event count",
            case.inserted_events,
            case.validator_count,
        );
        check_min(
            &mut failures,
            &case.name,
            "round count",
            report.rounds.len(),
            case.min_rounds,
        );
        check_min(
            &mut failures,
            &case.name,
            "famous witness count",
            report.famous_witnesses.len(),
            case.min_famous_witnesses,
        );
        check_min(
            &mut failures,
            &case.name,
            "finalized event count",
            report.finalized.len(),
            case.min_finalized,
        );
        check_max(
            &mut failures,
            &case.name,
            "undecided witness count",
            report.undecided_witnesses.len(),
            case.max_undecided_witnesses,
        );
        check_max(
            &mut failures,
            &case.name,
            "elapsed milliseconds",
            case.elapsed_ms,
            case.max_elapsed_ms,
        );

        if case.require_strict_finalized_order
            && !report
                .finalized
                .windows(2)
                .all(|window| window[0].index < window[1].index)
        {
            failures.push(EvalFailure {
                case: case.name.clone(),
                expectation: "strictly increasing finalized indexes".to_string(),
                observed: serde_json::json!(
                    report
                        .finalized
                        .iter()
                        .map(|event| event.index)
                        .collect::<Vec<_>>()
                ),
            });
        }

        if failures.len() == failures_before {
            passed += 1;
        }
    }

    ConsensusLoadEvalReport {
        cases: cases.len(),
        passed,
        total_events: event_total,
        average_events: average(event_total, cases.len()),
        average_finalized: average(finalized_total, cases.len()),
        max_elapsed_ms,
        failures,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeRoomEvalCase {
    pub name: String,
    pub room: RoomView,
    pub min_durable_messages: usize,
    pub expected_active_sessions: usize,
    pub expected_participants: usize,
    pub expected_counters: BTreeMap<String, i64>,
    pub expected_registers: BTreeMap<String, Value>,
    pub expected_sets: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeRoomEvalReport {
    pub cases: usize,
    pub passed: usize,
    pub average_durable_messages: f64,
    pub failures: Vec<EvalFailure>,
}

impl RealtimeRoomEvalReport {
    pub fn assert_passed(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "Realtime room evaluation failed {} of {} cases: {}",
                self.failures.len(),
                self.cases,
                failure_names(&self.failures)
            )))
        }
    }
}

pub fn evaluate_realtime_rooms(cases: &[RealtimeRoomEvalCase]) -> RealtimeRoomEvalReport {
    let mut failures = Vec::new();
    let mut passed = 0usize;
    let mut durable_total = 0usize;

    for case in cases {
        let failures_before = failures.len();
        let room = &case.room;
        durable_total += room.durable_messages.len();

        check_min(
            &mut failures,
            &case.name,
            "durable message count",
            room.durable_messages.len(),
            case.min_durable_messages,
        );
        check_eq(
            &mut failures,
            &case.name,
            "active session count",
            room.presence.active_sessions.len(),
            case.expected_active_sessions,
        );
        check_eq(
            &mut failures,
            &case.name,
            "participant count",
            room.presence.participants.len(),
            case.expected_participants,
        );

        for (key, expected) in &case.expected_counters {
            let observed = room.state.counters.get(key).copied();
            if observed != Some(*expected) {
                failures.push(EvalFailure {
                    case: case.name.clone(),
                    expectation: format!("counter {key} == {expected}"),
                    observed: serde_json::json!(observed),
                });
            }
        }
        for (key, expected) in &case.expected_registers {
            let observed = room.state.registers.get(key).map(|entry| &entry.value);
            if observed != Some(expected) {
                failures.push(EvalFailure {
                    case: case.name.clone(),
                    expectation: format!("register {key} == {expected}"),
                    observed: serde_json::json!(observed),
                });
            }
        }
        for (key, expected) in &case.expected_sets {
            let observed = room.state.sets.get(key);
            if observed != Some(expected) {
                failures.push(EvalFailure {
                    case: case.name.clone(),
                    expectation: format!("set {key} == {:?}", expected),
                    observed: serde_json::json!(observed),
                });
            }
        }

        if failures.len() == failures_before {
            passed += 1;
        }
    }

    RealtimeRoomEvalReport {
        cases: cases.len(),
        passed,
        average_durable_messages: average(durable_total, cases.len()),
        failures,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeLoadEvalCase {
    pub name: String,
    pub room: RoomView,
    pub opened_sessions: usize,
    pub published_messages: usize,
    pub elapsed_ms: u64,
    pub min_durable_messages: usize,
    pub expected_active_sessions: usize,
    pub expected_participants: usize,
    pub expected_counter_totals: BTreeMap<String, i64>,
    pub max_elapsed_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeLoadEvalReport {
    pub cases: usize,
    pub passed: usize,
    pub total_sessions: usize,
    pub total_messages: usize,
    pub average_messages: f64,
    pub max_elapsed_ms: u64,
    pub failures: Vec<EvalFailure>,
}

impl RealtimeLoadEvalReport {
    pub fn assert_passed(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "Realtime load evaluation failed {} of {} cases: {}",
                self.failures.len(),
                self.cases,
                failure_names(&self.failures)
            )))
        }
    }
}

pub fn evaluate_realtime_load(cases: &[RealtimeLoadEvalCase]) -> RealtimeLoadEvalReport {
    let mut failures = Vec::new();
    let mut passed = 0usize;
    let mut session_total = 0usize;
    let mut message_total = 0usize;
    let mut max_elapsed_ms = 0u64;

    for case in cases {
        let failures_before = failures.len();
        let room = &case.room;
        session_total += case.opened_sessions;
        message_total += case.published_messages;
        max_elapsed_ms = max_elapsed_ms.max(case.elapsed_ms);

        check_min(
            &mut failures,
            &case.name,
            "opened session count",
            case.opened_sessions,
            case.expected_participants,
        );
        check_min(
            &mut failures,
            &case.name,
            "published message count",
            case.published_messages,
            case.min_durable_messages,
        );
        check_min(
            &mut failures,
            &case.name,
            "durable message count",
            room.durable_messages.len(),
            case.min_durable_messages,
        );
        check_eq(
            &mut failures,
            &case.name,
            "active session count",
            room.presence.active_sessions.len(),
            case.expected_active_sessions,
        );
        check_eq(
            &mut failures,
            &case.name,
            "participant count",
            room.presence.participants.len(),
            case.expected_participants,
        );
        check_max(
            &mut failures,
            &case.name,
            "elapsed milliseconds",
            case.elapsed_ms,
            case.max_elapsed_ms,
        );

        for (key, expected) in &case.expected_counter_totals {
            let observed = room.state.counters.get(key).copied();
            if observed != Some(*expected) {
                failures.push(EvalFailure {
                    case: case.name.clone(),
                    expectation: format!("counter {key} == {expected}"),
                    observed: serde_json::json!(observed),
                });
            }
        }

        if failures.len() == failures_before {
            passed += 1;
        }
    }

    RealtimeLoadEvalReport {
        cases: cases.len(),
        passed,
        total_sessions: session_total,
        total_messages: message_total,
        average_messages: average(message_total, cases.len()),
        max_elapsed_ms,
        failures,
    }
}

fn failure(case: &str, expectation: String, observed: &Value) -> EvalFailure {
    EvalFailure {
        case: case.to_string(),
        expectation,
        observed: observed.clone(),
    }
}

fn check_eq<T>(failures: &mut Vec<EvalFailure>, case: &str, label: &str, observed: T, expected: T)
where
    T: Copy + Eq + Serialize,
{
    if observed != expected {
        failures.push(EvalFailure {
            case: case.to_string(),
            expectation: format!("{label} == {}", json_display(expected)),
            observed: serde_json::json!(observed),
        });
    }
}

fn check_min<T>(failures: &mut Vec<EvalFailure>, case: &str, label: &str, observed: T, minimum: T)
where
    T: Copy + Ord + Serialize,
{
    if observed < minimum {
        failures.push(EvalFailure {
            case: case.to_string(),
            expectation: format!("{label} >= {}", json_display(minimum)),
            observed: serde_json::json!(observed),
        });
    }
}

fn check_max<T>(failures: &mut Vec<EvalFailure>, case: &str, label: &str, observed: T, maximum: T)
where
    T: Copy + Ord + Serialize,
{
    if observed > maximum {
        failures.push(EvalFailure {
            case: case.to_string(),
            expectation: format!("{label} <= {}", json_display(maximum)),
            observed: serde_json::json!(observed),
        });
    }
}

fn json_display<T>(value: T) -> String
where
    T: Serialize,
{
    serde_json::to_value(value)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "<unserializable>".to_string())
}

fn average(total: usize, count: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

fn provider_name(provider: &ProviderVersion) -> String {
    format!(
        "{}/{}/{}",
        provider.provider, provider.model, provider.version
    )
}

fn failure_names(failures: &[EvalFailure]) -> String {
    failures
        .iter()
        .map(|failure| format!("{} ({})", failure.case, failure.expectation))
        .collect::<Vec<_>>()
        .join(", ")
}
