use babble_lens::{RankedCandidate, Reason};
use babble_types::{IdentityId, ObjectId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod sync;
pub use sync::*;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalUserModel {
    pub model_revision: Option<String>,
    pub interests: Vec<String>,
    pub expertise: Vec<String>,
    pub muted_terms: Vec<String>,
    pub hidden_terms: Vec<String>,
    pub hidden_authors: BTreeSet<IdentityId>,
    pub creator_affinity: BTreeMap<IdentityId, f64>,
    pub seen_objects: BTreeMap<ObjectId, u32>,
    pub novelty_tolerance: f64,
    pub exploration_preference: f64,
    pub evidence_preference: f64,
    pub contradiction_tolerance: f64,
}

impl LocalUserModel {
    pub fn normalized(&self) -> Self {
        Self {
            model_revision: self.model_revision.clone(),
            interests: normalized_terms(&self.interests),
            expertise: normalized_terms(&self.expertise),
            muted_terms: normalized_terms(&self.muted_terms),
            hidden_terms: normalized_terms(&self.hidden_terms),
            hidden_authors: self.hidden_authors.clone(),
            creator_affinity: self
                .creator_affinity
                .iter()
                .map(|(author, score)| (author.clone(), clamp(*score)))
                .collect(),
            seen_objects: self
                .seen_objects
                .iter()
                .filter_map(|(object_id, count)| {
                    if *count == 0 {
                        None
                    } else {
                        Some((object_id.clone(), *count))
                    }
                })
                .collect(),
            novelty_tolerance: clamp(self.novelty_tolerance),
            exploration_preference: clamp(self.exploration_preference),
            evidence_preference: clamp(self.evidence_preference),
            contradiction_tolerance: clamp(self.contradiction_tolerance),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationObjectSummary {
    pub object_id: ObjectId,
    pub author: IdentityId,
    pub kind: String,
    pub text: String,
    pub topics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizedCandidate {
    pub ranked: RankedCandidate,
    pub public_score: f64,
    pub personalized_score: f64,
    pub reasons: Vec<Reason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FilteredCandidate {
    pub object_id: ObjectId,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationTrace {
    pub privacy_boundary: String,
    pub model_revision: Option<String>,
    pub ranked: Vec<PersonalizedCandidate>,
    pub filtered: Vec<FilteredCandidate>,
}

#[derive(Clone, Debug, Default)]
pub struct LocalPersonalizer;

impl LocalPersonalizer {
    pub fn personalize(
        &self,
        model: &LocalUserModel,
        candidates: &[RankedCandidate],
        summaries: &BTreeMap<ObjectId, PersonalizationObjectSummary>,
    ) -> PersonalizationTrace {
        let model = model.normalized();
        let max_public_score = candidates
            .iter()
            .map(|candidate| finite_positive(candidate.score))
            .fold(0.0, f64::max)
            .max(1.0);
        let mut ranked = Vec::new();
        let mut filtered = Vec::new();

        for candidate in candidates {
            let object_id = &candidate.candidate.object_id;
            let summary = summaries.get(object_id);
            let filter_reasons = filter_reasons(&model, summary);
            if !filter_reasons.is_empty() {
                filtered.push(FilteredCandidate {
                    object_id: object_id.clone(),
                    reasons: filter_reasons,
                });
                continue;
            }

            let public_score = finite_positive(candidate.score) / max_public_score;
            let reasons = personalization_reasons(&model, candidate, summary, public_score);
            let personalized_score = clamp(reasons.iter().map(|reason| reason.contribution).sum());
            ranked.push(PersonalizedCandidate {
                ranked: candidate.clone(),
                public_score,
                personalized_score,
                reasons,
            });
        }

        ranked.sort_by(|left, right| {
            right
                .personalized_score
                .partial_cmp(&left.personalized_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    right
                        .ranked
                        .candidate
                        .created_at
                        .cmp(&left.ranked.candidate.created_at)
                })
                .then_with(|| {
                    left.ranked
                        .candidate
                        .object_id
                        .cmp(&right.ranked.candidate.object_id)
                })
        });
        filtered.sort_by(|left, right| left.object_id.cmp(&right.object_id));

        PersonalizationTrace {
            privacy_boundary: "local_only".to_string(),
            model_revision: model.model_revision,
            ranked,
            filtered,
        }
    }
}

fn personalization_reasons(
    model: &LocalUserModel,
    candidate: &RankedCandidate,
    summary: Option<&PersonalizationObjectSummary>,
    public_score: f64,
) -> Vec<Reason> {
    let terms = summary_terms(summary);
    let interest_match = term_overlap(&model.interests, &terms);
    let expertise_match = term_overlap(&model.expertise, &terms);
    let author_affinity = summary
        .and_then(|summary| model.creator_affinity.get(&summary.author))
        .copied()
        .unwrap_or_default();
    let seen_count = model
        .seen_objects
        .get(&candidate.candidate.object_id)
        .copied()
        .unwrap_or_default();
    let saturation_penalty = (seen_count as f64 / 6.0).min(0.18);
    let novelty_fit = 1.0 - (candidate.candidate.signals.novelty - model.novelty_tolerance).abs();
    let contradiction_penalty =
        if candidate.candidate.signals.contradiction > model.contradiction_tolerance {
            (candidate.candidate.signals.contradiction - model.contradiction_tolerance) * 0.18
        } else {
            0.0
        };

    vec![
        reason("public_lens_score", 0.44 * public_score),
        reason("private.interest_match", 0.18 * interest_match),
        reason("private.expertise_match", 0.09 * expertise_match),
        reason("private.creator_affinity", 0.08 * author_affinity),
        reason("private.novelty_fit", 0.08 * clamp(novelty_fit)),
        reason(
            "private.exploration_preference",
            0.06 * model.exploration_preference * candidate.candidate.signals.exploration,
        ),
        reason(
            "private.evidence_preference",
            0.05 * model.evidence_preference * candidate.candidate.signals.evidence_quality,
        ),
        reason("private.saturation_penalty", -saturation_penalty),
        reason("private.contradiction_penalty", -contradiction_penalty),
    ]
}

fn filter_reasons(
    model: &LocalUserModel,
    summary: Option<&PersonalizationObjectSummary>,
) -> Vec<String> {
    let Some(summary) = summary else {
        return Vec::new();
    };
    let terms = summary_terms(Some(summary));
    let mut reasons = Vec::new();
    if model.hidden_authors.contains(&summary.author) {
        reasons.push("private.hidden_author".to_string());
    }
    if intersects(&model.hidden_terms, &terms) {
        reasons.push("private.hidden_term".to_string());
    }
    if intersects(&model.muted_terms, &terms) {
        reasons.push("private.muted_term".to_string());
    }
    reasons
}

fn summary_terms(summary: Option<&PersonalizationObjectSummary>) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    let Some(summary) = summary else {
        return terms;
    };
    for token in tokenize(&summary.kind) {
        terms.insert(token);
    }
    for token in tokenize(&summary.text) {
        terms.insert(token);
    }
    for topic in &summary.topics {
        for token in tokenize(topic) {
            terms.insert(token);
        }
    }
    terms
}

fn normalized_terms(values: &[String]) -> Vec<String> {
    let mut terms = values
        .iter()
        .flat_map(|value| tokenize(value))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    terms.truncate(128);
    terms
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter_map(|term| {
            let term = term.trim().to_ascii_lowercase();
            if term.len() < 2 { None } else { Some(term) }
        })
        .collect()
}

fn term_overlap(private_terms: &[String], object_terms: &BTreeSet<String>) -> f64 {
    if private_terms.is_empty() || object_terms.is_empty() {
        return 0.0;
    }
    let matches = private_terms
        .iter()
        .filter(|term| object_terms.contains(*term))
        .count();
    (matches as f64 / private_terms.len().max(1) as f64).clamp(0.0, 1.0)
}

fn intersects(private_terms: &[String], object_terms: &BTreeSet<String>) -> bool {
    private_terms.iter().any(|term| object_terms.contains(term))
}

fn reason(signal: &str, contribution: f64) -> Reason {
    Reason {
        signal: signal.to_string(),
        contribution,
    }
}

fn finite_positive(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn clamp(value: f64) -> f64 {
    if value.is_nan() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use babble_lens::{
        Candidate, CandidateSource, CandidateSourceContribution, EvidenceSignals,
        ReputationSignals, Signals,
    };
    use babble_types::{Hash, Timestamp};
    use time::OffsetDateTime;

    #[test]
    fn local_model_reranks_without_exposing_raw_private_terms() {
        let author = identity_id("alice");
        let distant_author = identity_id("distant");
        let local_model = LocalUserModel {
            model_revision: Some("local-device-rev-7".to_string()),
            interests: vec!["crdt collaborative canvas".to_string()],
            expertise: vec!["distributed systems".to_string()],
            creator_affinity: BTreeMap::from([(author.clone(), 0.9)]),
            novelty_tolerance: 0.7,
            exploration_preference: 0.8,
            evidence_preference: 0.9,
            contradiction_tolerance: 0.4,
            ..Default::default()
        };
        let crdt = object_id("crdt");
        let celebrity = object_id("celebrity");
        let candidates = vec![
            ranked(celebrity.clone(), 0.92, 10, 0.40, 0.20, 0.4),
            ranked(crdt.clone(), 0.50, 20, 0.75, 0.80, 0.8),
        ];
        let summaries = BTreeMap::from([
            (
                celebrity.clone(),
                summary(celebrity, distant_author, "celebrity recap", &["pop"]),
            ),
            (
                crdt.clone(),
                summary(
                    crdt.clone(),
                    author,
                    "collaborative CRDT canvas for distributed systems",
                    &["protocol"],
                ),
            ),
        ]);

        let trace = LocalPersonalizer.personalize(&local_model, &candidates, &summaries);

        assert_eq!(trace.privacy_boundary, "local_only");
        assert_eq!(trace.model_revision.as_deref(), Some("local-device-rev-7"));
        assert_eq!(trace.ranked[0].ranked.candidate.object_id, crdt);
        let serialized = serde_json::to_string(&trace).unwrap();
        assert!(!serialized.contains("distributed systems"));
        assert!(serialized.contains("private.interest_match"));
    }

    #[test]
    fn local_filters_remove_hidden_terms_and_authors() {
        let hidden_author = identity_id("blocked");
        let hidden_object = object_id("hidden");
        let muted_object = object_id("muted");
        let visible_object = object_id("visible");
        let model = LocalUserModel {
            hidden_terms: vec!["spoiler".to_string()],
            muted_terms: vec!["ragebait".to_string()],
            hidden_authors: BTreeSet::from([hidden_author.clone()]),
            novelty_tolerance: 0.5,
            exploration_preference: 0.5,
            evidence_preference: 0.5,
            contradiction_tolerance: 1.0,
            ..Default::default()
        };
        let candidates = vec![
            ranked(hidden_object.clone(), 0.8, 10, 0.5, 0.5, 0.5),
            ranked(muted_object.clone(), 0.7, 20, 0.5, 0.5, 0.5),
            ranked(visible_object.clone(), 0.6, 30, 0.5, 0.5, 0.5),
        ];
        let summaries = BTreeMap::from([
            (
                hidden_object.clone(),
                summary(hidden_object.clone(), hidden_author, "neutral text", &[]),
            ),
            (
                muted_object.clone(),
                summary(
                    muted_object.clone(),
                    identity_id("ok"),
                    "ragebait spoiler",
                    &[],
                ),
            ),
            (
                visible_object.clone(),
                summary(
                    visible_object.clone(),
                    identity_id("ok"),
                    "plain protocol note",
                    &[],
                ),
            ),
        ]);

        let trace = LocalPersonalizer.personalize(&model, &candidates, &summaries);

        assert_eq!(trace.ranked.len(), 1);
        assert_eq!(trace.ranked[0].ranked.candidate.object_id, visible_object);
        assert_eq!(trace.filtered.len(), 2);
        assert!(trace.filtered.iter().any(|filtered| {
            filtered.object_id == hidden_object
                && filtered
                    .reasons
                    .contains(&"private.hidden_author".to_string())
        }));
        assert!(trace.filtered.iter().any(|filtered| {
            filtered.object_id == muted_object
                && filtered
                    .reasons
                    .contains(&"private.hidden_term".to_string())
                && filtered.reasons.contains(&"private.muted_term".to_string())
        }));
    }

    #[test]
    fn personalization_is_deterministic_for_equal_scores() {
        let model = LocalUserModel {
            novelty_tolerance: 0.5,
            exploration_preference: 0.5,
            evidence_preference: 0.5,
            contradiction_tolerance: 0.5,
            ..Default::default()
        };
        let first = object_id("a");
        let second = object_id("b");
        let candidates = vec![
            ranked(second.clone(), 0.5, 10, 0.5, 0.5, 0.5),
            ranked(first.clone(), 0.5, 10, 0.5, 0.5, 0.5),
        ];

        let trace = LocalPersonalizer.personalize(&model, &candidates, &BTreeMap::new());
        let mut expected = [first.clone(), second.clone()];
        expected.sort();

        assert_eq!(trace.ranked[0].ranked.candidate.object_id, expected[0]);
        assert_eq!(trace.ranked[1].ranked.candidate.object_id, expected[1]);
    }

    fn ranked(
        object_id: ObjectId,
        score: f64,
        seconds: i64,
        novelty: f64,
        exploration: f64,
        evidence_quality: f64,
    ) -> RankedCandidate {
        RankedCandidate {
            candidate: Candidate {
                object_id,
                source: CandidateSource::Exploration,
                sources: vec![CandidateSourceContribution {
                    source: CandidateSource::Exploration,
                    weight: 1.0,
                }],
                created_at: Timestamp(OffsetDateTime::from_unix_timestamp(seconds).unwrap()),
                signals: Signals {
                    social_distance: 0.5,
                    followed_author: false,
                    relevance: 0.5,
                    novelty,
                    evidence_quality,
                    contradiction: 0.0,
                    evidence: EvidenceSignals::default(),
                    reputation: ReputationSignals::default(),
                    temporal: 0.5,
                    exploration,
                },
            },
            score,
            reasons: Vec::new(),
        }
    }

    fn summary(
        object_id: ObjectId,
        author: IdentityId,
        text: &str,
        topics: &[&str],
    ) -> PersonalizationObjectSummary {
        PersonalizationObjectSummary {
            object_id,
            author,
            kind: "babble.text.v1".to_string(),
            text: text.to_string(),
            topics: topics.iter().map(|topic| (*topic).to_string()).collect(),
        }
    }

    fn object_id(seed: &str) -> ObjectId {
        ObjectId::from_hash(&Hash::from_bytes(seed.as_bytes()))
    }

    fn identity_id(seed: &str) -> IdentityId {
        IdentityId::from_hash(&Hash::from_bytes(seed.as_bytes()))
    }
}
