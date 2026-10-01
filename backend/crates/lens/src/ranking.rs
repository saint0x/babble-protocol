//! Canonical v1 ranking boundary, independent of provider transport.
//!
//! Empty and all-zero lens stacks use Following. Diversity floors and source
//! shares are soft score adjustments, not selection guarantees. Ranking traces
//! contain every input; diversity traces contain the selected prefix and the
//! remaining IDs in original lens order.

use crate::{
    Candidate, DiversityPolicy, DiversityTrace, LensStack, RankedCandidate, RankingTrace, Reason,
    diversify_ranked, normalized_weights,
};
use babble_types::{Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankingRequest {
    pub candidates: Vec<Candidate>,
    pub lens: LensStack,
    pub diversity: DiversityPolicy,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankingResult {
    pub ranked: Vec<RankedCandidate>,
    pub trace: RankingTrace,
    pub diversity_trace: DiversityTrace,
    pub provider: RankingProviderVersion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankingProviderVersion {
    pub provider: String,
    pub model: String,
    pub version: String,
}

pub trait RankingProvider: Send + Sync {
    fn version(&self) -> RankingProviderVersion;
    fn rank(&self, request: &RankingRequest) -> Result<RankingResult>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRanker;

impl RankingProvider for NativeRanker {
    fn version(&self) -> RankingProviderVersion {
        RankingProviderVersion {
            provider: "babble-rust".into(),
            model: "lenses-v1".into(),
            version: "1".into(),
        }
    }

    fn rank(&self, request: &RankingRequest) -> Result<RankingResult> {
        request.validate()?;
        let (ranked, trace) = request.lens.rank_with_trace(&request.candidates);
        let (ranked, diversity_trace) =
            diversify_ranked(&ranked, &request.diversity, request.limit);
        let result = RankingResult {
            ranked,
            trace,
            diversity_trace,
            provider: self.version(),
        };
        result.validate_for(request, &self.version())?;
        Ok(result)
    }
}

impl RankingRequest {
    pub fn validate(&self) -> Result<()> {
        require(self.candidates.len() <= 200, "at most 200 candidates")?;
        require((1..=200).contains(&self.limit), "limit must be 1..200")?;
        require(
            visible(&self.lens.id),
            "stack ID must be 1..128 visible ASCII bytes",
        )?;
        require(self.lens.weights.len() <= 8, "at most eight lens weights")?;
        let mut lenses = BTreeSet::new();
        for weight in &self.lens.weights {
            require(lenses.insert(weight.lens.id()), "duplicate lens")?;
            number(weight.weight, 0.0, f64::MAX, "lens weight")?;
        }
        number(
            self.diversity.max_source_share,
            0.0,
            1.0,
            "maximum source share",
        )?;
        require(
            self.diversity.source_floors.len() <= 8,
            "at most eight source floors",
        )?;
        let mut floors = BTreeSet::new();
        for floor in &self.diversity.source_floors {
            require(floors.insert(&floor.source), "duplicate source floor")?;
            require(floor.minimum <= 200, "source minimum exceeds 200")?;
        }
        let mut ids = BTreeSet::new();
        for candidate in &self.candidates {
            candidate.object_id.validate()?;
            require(
                candidate.object_id.as_str()[4..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "object ID must contain a lowercase hexadecimal hash",
            )?;
            require(ids.insert(&candidate.object_id), "duplicate candidate ID")?;
            require(
                (0..=9999).contains(&candidate.created_at.0.year())
                    && candidate.created_at.0.offset().seconds_past_minute() == 0,
                "timestamp must be RFC3339 serializable",
            )?;
            require(
                !candidate.sources.is_empty() && candidate.sources.len() <= 8,
                "candidate must have 1..8 sources",
            )?;
            let mut sources = BTreeSet::new();
            for source in &candidate.sources {
                require(sources.insert(&source.source), "duplicate candidate source")?;
                number(source.weight, 0.0, 1.0, "source weight")?;
            }
            require(
                sources.contains(&candidate.source),
                "primary source missing from contributions",
            )?;
            let s = &candidate.signals;
            for value in [
                s.social_distance,
                s.relevance,
                s.novelty,
                s.evidence_quality,
                s.contradiction,
                s.temporal,
                s.exploration,
                s.reputation.epistemic_accuracy,
                s.reputation.evidence_quality,
                s.reputation.social_constructiveness,
                s.reputation.creative_contribution,
                s.reputation.moderation,
                s.reputation.domain_expertise,
            ] {
                number(value, 0.0, 1.0, "candidate signal")?;
            }
            for value in [
                s.evidence.human_support,
                s.evidence.judgment_support,
                s.evidence.human_contradiction,
                s.evidence.judgment_contradiction,
            ] {
                number(value, 0.0, f64::MAX, "evidence count")?;
            }
        }
        Ok(())
    }
}

impl RankingResult {
    /// Check bounded structure and numeric consistency without rerunning scoring
    /// or diversity selection. This is not proof that a provider implements the
    /// model: cross-provider golden tests establish that independently.
    pub fn validate_for(
        &self,
        request: &RankingRequest,
        expected_provider: &RankingProviderVersion,
    ) -> Result<()> {
        request.validate()?;
        require(
            &self.provider == expected_provider,
            "unexpected ranking provider",
        )?;
        for value in [
            &self.provider.provider,
            &self.provider.model,
            &self.provider.version,
        ] {
            require(
                visible(value),
                "provider identity must be 1..128 visible ASCII bytes",
            )?;
        }
        let inputs: BTreeMap<_, _> = request
            .candidates
            .iter()
            .map(|c| (&c.object_id, c))
            .collect();
        let selected_len = request.limit.min(inputs.len());
        require(
            self.ranked.len() == selected_len
                && self.diversity_trace.candidates.len() == selected_len,
            "selected result length mismatch",
        )?;
        require(
            self.trace.stack_id == request.lens.id && self.trace.candidates.len() == inputs.len(),
            "ranking trace scope mismatch",
        )?;
        require(
            self.diversity_trace.policy == request.diversity.bounded(),
            "diversity policy mismatch",
        )?;
        require(
            self.diversity_trace.filtered.len() == inputs.len() - selected_len,
            "filtered result length mismatch",
        )?;
        let weights = normalized_weights(&request.lens.weights);
        let definitions: Vec<_> = weights.iter().map(|w| w.lens.definition()).collect();
        let mut traces = BTreeMap::new();
        for (index, trace) in self.trace.candidates.iter().enumerate() {
            let input = inputs
                .get(&trace.object_id)
                .ok_or_else(|| invalid("unknown trace candidate"))?;
            require(
                traces.insert(&trace.object_id, trace).is_none(),
                "duplicate trace candidate",
            )?;
            require(
                trace.rank == index + 1
                    && trace.source == input.source
                    && trace.sources == input.sources,
                "trace identity or rank mismatch",
            )?;
            // Weird intentionally sums to as much as 1.1 before diversification.
            number(trace.score, 0.0, 1.1 + 1e-12, "lens score")?;
            require(
                trace.lens_contributions.len() == weights.len(),
                "lens contribution count mismatch",
            )?;
            for ((part, weight), definition) in trace
                .lens_contributions
                .iter()
                .zip(&weights)
                .zip(&definitions)
            {
                require(
                    part.lens_id == weight.lens.id(),
                    "lens contribution identity mismatch",
                )?;
                number(part.weight, 0.0, 1.0, "normalized lens weight")?;
                equal(
                    part.weight,
                    weight.weight,
                    "normalized lens weight mismatch",
                )?;
                number(
                    part.score,
                    0.0,
                    1.1 * part.weight + 1e-12,
                    "lens contribution score",
                )?;
                require(
                    part.reasons.len() == definition.required_signals.len(),
                    "lens reason count mismatch",
                )?;
                for (reason, signal) in part.reasons.iter().zip(&definition.required_signals) {
                    require(&reason.signal == signal, "lens reason identity mismatch")?;
                    number(
                        reason.contribution,
                        0.0,
                        1.1 + 1e-12,
                        "lens reason contribution",
                    )?;
                }
                equal(
                    part.score,
                    part.reasons.iter().map(|r| r.contribution).sum(),
                    "lens reason sum mismatch",
                )?;
            }
            equal(
                trace.score,
                trace.lens_contributions.iter().map(|p| p.score).sum(),
                "lens contribution sum mismatch",
            )?;
            if index > 0 {
                let previous = &self.trace.candidates[index - 1];
                let previous_input = inputs[&previous.object_id];
                let order = previous
                    .score
                    .partial_cmp(&trace.score)
                    .unwrap()
                    .then_with(|| previous_input.created_at.cmp(&input.created_at))
                    .then_with(|| trace.object_id.cmp(&previous.object_id));
                require(!order.is_lt(), "ranking trace order mismatch")?;
            }
        }
        let mut selected = BTreeSet::new();
        for (index, (ranked, diversity)) in self
            .ranked
            .iter()
            .zip(&self.diversity_trace.candidates)
            .enumerate()
        {
            let id = &ranked.candidate.object_id;
            let input = inputs
                .get(id)
                .ok_or_else(|| invalid("unknown selected candidate"))?;
            require(
                &ranked.candidate == *input
                    && ranked.candidate.created_at.0.offset() == input.created_at.0.offset(),
                "provider changed input candidate",
            )?;
            require(selected.insert(id), "duplicate selected candidate")?;
            let trace = traces[id];
            require(
                diversity.rank == index + 1
                    && &diversity.object_id == id
                    && diversity.source == input.source,
                "diversity trace identity or order mismatch",
            )?;
            number(ranked.score, 0.0, 1.0, "ranked score")?;
            number(
                diversity.lens_score,
                0.0,
                1.1 + 1e-12,
                "diversity lens score",
            )?;
            number(diversity.diversified_score, 0.0, 1.0, "diversified score")?;
            equal(
                diversity.lens_score,
                trace.score,
                "diversity lens score mismatch",
            )?;
            equal(
                diversity.diversified_score,
                ranked.score,
                "diversified score mismatch",
            )?;
            require(diversity.reasons.len() <= 2, "too many diversity reasons")?;
            let mut seen = BTreeSet::new();
            for reason in &diversity.reasons {
                require(
                    seen.insert(reason.signal.as_str()),
                    "duplicate diversity reason",
                )?;
                match reason.signal.as_str() {
                    "source_floor" => {
                        number(reason.contribution, f64::MIN_POSITIVE, 0.18, "floor bonus")?
                    }
                    "source_concentration" => {
                        number(reason.contribution, -0.25, 0.0, "concentration penalty")?;
                        require(reason.contribution < 0.0, "zero concentration penalty")?;
                    }
                    _ => return Err(invalid("unknown diversity reason")),
                }
            }
            require(
                index != 0 || diversity.reasons.is_empty(),
                "first selection has diversity adjustment",
            )?;
            require(
                diversity.reasons.len() < 2 || diversity.reasons[0].signal == "source_floor",
                "diversity reason order mismatch",
            )?;
            equal(
                ranked.score,
                (trace.score
                    + diversity
                        .reasons
                        .iter()
                        .map(|r| r.contribution)
                        .sum::<f64>())
                .clamp(0.0, 1.0),
                "diversity reason sum mismatch",
            )?;
            let expected_reasons: Vec<Reason> = trace
                .lens_contributions
                .iter()
                .flat_map(|part| {
                    part.reasons.iter().map(move |r| Reason {
                        signal: format!("{}:{}", part.lens_id, r.signal),
                        contribution: r.contribution,
                    })
                })
                .chain(diversity.reasons.iter().map(|r| Reason {
                    signal: format!("diversity:{}", r.signal),
                    contribution: r.contribution,
                }))
                .collect();
            require(
                ranked.reasons.len() == expected_reasons.len(),
                "ranked reason count mismatch",
            )?;
            for (actual, expected) in ranked.reasons.iter().zip(&expected_reasons) {
                require(
                    actual.signal == expected.signal,
                    "ranked reason identity mismatch",
                )?;
                number(
                    actual.contribution,
                    -0.25,
                    1.1 + 1e-12,
                    "ranked reason contribution",
                )?;
                equal(
                    actual.contribution,
                    expected.contribution,
                    "ranked reason contribution mismatch",
                )?;
            }
        }
        let expected_filtered: Vec<_> = self
            .trace
            .candidates
            .iter()
            .filter(|t| !selected.contains(&t.object_id))
            .map(|t| &t.object_id)
            .collect();
        require(
            self.diversity_trace.filtered.iter().eq(expected_filtered),
            "filtered IDs or order mismatch",
        )?;
        Ok(())
    }
}

fn visible(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_graphic())
}

fn invalid(message: &str) -> Error {
    Error::Canonical(format!("invalid ranking contract: {message}"))
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}

fn number(value: f64, minimum: f64, maximum: f64, message: &str) -> Result<()> {
    require(
        value.is_finite() && (minimum..=maximum).contains(&value),
        message,
    )
}

fn equal(left: f64, right: f64, message: &str) -> Result<()> {
    require(
        left.is_finite() && right.is_finite() && (left - right).abs() <= 1e-12,
        message,
    )
}
