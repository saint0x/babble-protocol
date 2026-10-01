use babble_types::{ObjectId, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub mod ranking;
pub use ranking::{
    NativeRanker, RankingProvider, RankingProviderVersion, RankingRequest, RankingResult,
};

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum CandidateSource {
    Following,
    SocialGraph,
    SemanticNeighborhood,
    Temporal,
    Emerging,
    Evidence,
    Contradiction,
    Exploration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Signals {
    pub social_distance: f64,
    pub followed_author: bool,
    pub relevance: f64,
    pub novelty: f64,
    pub evidence_quality: f64,
    pub contradiction: f64,
    pub evidence: EvidenceSignals,
    pub reputation: ReputationSignals,
    pub temporal: f64,
    pub exploration: f64,
}

impl Signals {
    pub fn bounded(self) -> Self {
        Self {
            social_distance: clamp(self.social_distance),
            followed_author: self.followed_author,
            relevance: clamp(self.relevance),
            novelty: clamp(self.novelty),
            evidence_quality: clamp(self.evidence_quality),
            contradiction: clamp(self.contradiction),
            evidence: self.evidence.bounded(),
            reputation: self.reputation.bounded(),
            temporal: clamp(self.temporal),
            exploration: clamp(self.exploration),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSignals {
    pub human_support: f64,
    pub judgment_support: f64,
    pub human_contradiction: f64,
    pub judgment_contradiction: f64,
}

impl EvidenceSignals {
    pub fn bounded(self) -> Self {
        Self {
            human_support: positive(self.human_support),
            judgment_support: positive(self.judgment_support),
            human_contradiction: positive(self.human_contradiction),
            judgment_contradiction: positive(self.judgment_contradiction),
        }
    }

    pub fn support_score(&self) -> f64 {
        ((self.human_support + 0.75 * self.judgment_support) / 3.0).clamp(0.0, 1.0)
    }

    pub fn contradiction_score(&self) -> f64 {
        ((self.human_contradiction + 0.75 * self.judgment_contradiction) / 3.0).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReputationSignals {
    pub epistemic_accuracy: f64,
    pub evidence_quality: f64,
    pub social_constructiveness: f64,
    pub creative_contribution: f64,
    pub moderation: f64,
    pub domain_expertise: f64,
}

impl ReputationSignals {
    pub fn bounded(self) -> Self {
        Self {
            epistemic_accuracy: clamp(self.epistemic_accuracy),
            evidence_quality: clamp(self.evidence_quality),
            social_constructiveness: clamp(self.social_constructiveness),
            creative_contribution: clamp(self.creative_contribution),
            moderation: clamp(self.moderation),
            domain_expertise: clamp(self.domain_expertise),
        }
    }

    pub fn following_score(&self) -> f64 {
        clamp(0.70 * self.social_constructiveness + 0.30 * self.creative_contribution)
    }

    pub fn research_score(&self) -> f64 {
        clamp(
            0.35 * self.evidence_quality
                + 0.30 * self.domain_expertise
                + 0.25 * self.epistemic_accuracy
                + 0.10 * self.moderation,
        )
    }

    pub fn creative_score(&self) -> f64 {
        clamp(0.65 * self.creative_contribution + 0.35 * self.social_constructiveness)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateSourceContribution {
    pub source: CandidateSource,
    pub weight: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub object_id: ObjectId,
    pub source: CandidateSource,
    pub sources: Vec<CandidateSourceContribution>,
    pub created_at: Timestamp,
    pub signals: Signals,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankedCandidate {
    pub candidate: Candidate,
    pub score: f64,
    pub reasons: Vec<Reason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reason {
    pub signal: String,
    pub contribution: f64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum BuiltInLens {
    Following,
    Friends,
    Research,
    IntellectualSerendipity,
    Contradictions,
    Emerging,
    SlowInternet,
    Weird,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LensWeight {
    pub lens: BuiltInLens,
    pub weight: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LensStack {
    pub id: String,
    pub weights: Vec<LensWeight>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum LensExecution {
    LocalDeterministic,
    RemoteDeclarative,
    ExecutableSandboxed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LensDefinition {
    pub id: String,
    pub lens: BuiltInLens,
    pub version: u32,
    pub name: String,
    pub description: String,
    pub execution: LensExecution,
    pub required_signals: Vec<String>,
    pub required_sources: Vec<CandidateSource>,
    pub required_permissions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankingTrace {
    pub stack_id: String,
    pub candidates: Vec<CandidateTrace>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceFloor {
    pub source: CandidateSource,
    pub minimum: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiversityPolicy {
    pub max_source_share: f64,
    pub source_floors: Vec<SourceFloor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiversityReason {
    pub signal: String,
    pub contribution: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiversifiedCandidateTrace {
    pub rank: usize,
    pub object_id: ObjectId,
    pub source: CandidateSource,
    pub lens_score: f64,
    pub diversified_score: f64,
    pub reasons: Vec<DiversityReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiversityTrace {
    pub policy: DiversityPolicy,
    pub candidates: Vec<DiversifiedCandidateTrace>,
    pub filtered: Vec<ObjectId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateTrace {
    pub rank: usize,
    pub object_id: ObjectId,
    pub source: CandidateSource,
    pub sources: Vec<CandidateSourceContribution>,
    pub score: f64,
    pub lens_contributions: Vec<LensContribution>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LensContribution {
    pub lens_id: String,
    pub weight: f64,
    pub score: f64,
    pub reasons: Vec<Reason>,
}

pub trait Lens {
    fn id(&self) -> &'static str;
    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate>;
}

#[derive(Clone, Debug, Default)]
pub struct FollowingLens;

#[derive(Clone, Debug, Default)]
pub struct FriendsLens;

#[derive(Clone, Debug, Default)]
pub struct ResearchLens;

#[derive(Clone, Debug, Default)]
pub struct IntellectualSerendipityLens;

#[derive(Clone, Debug, Default)]
pub struct ContradictionsLens;

#[derive(Clone, Debug, Default)]
pub struct EmergingLens;

#[derive(Clone, Debug, Default)]
pub struct SlowInternetLens;

#[derive(Clone, Debug, Default)]
pub struct WeirdLens;

impl Lens for FollowingLens {
    fn id(&self) -> &'static str {
        "babble.lens.following.v1"
    }

    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        rank_with(candidates, |candidate| {
            let followed = if candidate.signals.followed_author {
                0.55
            } else {
                0.0
            };
            vec![
                reason("followed_author", followed),
                reason("temporal", 0.30 * candidate.signals.temporal),
                reason(
                    "reputation.social_constructiveness",
                    0.07 * candidate.signals.reputation.social_constructiveness,
                ),
                reason(
                    "reputation.creative_contribution",
                    0.03 * candidate.signals.reputation.creative_contribution,
                ),
                reason("relevance", 0.05 * candidate.signals.relevance),
            ]
        })
    }
}

impl Lens for FriendsLens {
    fn id(&self) -> &'static str {
        "babble.lens.friends.v1"
    }

    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        rank_with(candidates, |candidate| {
            vec![
                reason(
                    "social_distance",
                    0.40 * (1.0 - candidate.signals.social_distance),
                ),
                reason(
                    "followed_author",
                    if candidate.signals.followed_author {
                        0.22
                    } else {
                        0.0
                    },
                ),
                reason("temporal", 0.15 * candidate.signals.temporal),
                reason(
                    "reputation.social_constructiveness",
                    0.13 * candidate.signals.reputation.social_constructiveness,
                ),
                reason("relevance", 0.10 * candidate.signals.relevance),
            ]
        })
    }
}

impl Lens for ResearchLens {
    fn id(&self) -> &'static str {
        "babble.lens.research.v1"
    }

    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        rank_with(candidates, |candidate| {
            vec![
                reason(
                    "evidence_quality",
                    0.40 * candidate.signals.evidence_quality,
                ),
                reason("relevance", 0.25 * candidate.signals.relevance),
                reason("contradiction", 0.15 * candidate.signals.contradiction),
                reason(
                    "reputation.evidence_quality",
                    0.06 * candidate.signals.reputation.evidence_quality,
                ),
                reason(
                    "reputation.domain_expertise",
                    0.05 * candidate.signals.reputation.domain_expertise,
                ),
                reason(
                    "reputation.epistemic_accuracy",
                    0.04 * candidate.signals.reputation.epistemic_accuracy,
                ),
                reason("temporal", 0.05 * candidate.signals.temporal),
            ]
        })
    }
}

impl Lens for IntellectualSerendipityLens {
    fn id(&self) -> &'static str {
        "babble.lens.intellectual_serendipity.v1"
    }

    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        rank_with(candidates, |candidate| {
            let adjacent_relevance = 1.0 - (candidate.signals.relevance - 0.62).abs() / 0.62;
            vec![
                reason(
                    "adjacent_relevance",
                    0.25 * adjacent_relevance.clamp(0.0, 1.0),
                ),
                reason("novelty", 0.25 * candidate.signals.novelty),
                reason(
                    "evidence_quality",
                    0.18 * candidate.signals.evidence_quality,
                ),
                reason("exploration", 0.16 * candidate.signals.exploration),
                reason(
                    "reputation.research",
                    0.10 * candidate.signals.reputation.research_score(),
                ),
                reason("temporal", 0.06 * candidate.signals.temporal),
            ]
        })
    }
}

impl Lens for ContradictionsLens {
    fn id(&self) -> &'static str {
        "babble.lens.contradictions.v1"
    }

    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        rank_with(candidates, |candidate| {
            vec![
                reason("contradiction", 0.42 * candidate.signals.contradiction),
                reason(
                    "evidence.contradiction_score",
                    0.18 * candidate.signals.evidence.contradiction_score(),
                ),
                reason(
                    "evidence_quality",
                    0.16 * candidate.signals.evidence_quality,
                ),
                reason("relevance", 0.14 * candidate.signals.relevance),
                reason("novelty", 0.06 * candidate.signals.novelty),
                reason(
                    "source.contradiction",
                    source_bonus(candidate, CandidateSource::Contradiction, 0.04),
                ),
            ]
        })
    }
}

impl Lens for EmergingLens {
    fn id(&self) -> &'static str {
        "babble.lens.emerging.v1"
    }

    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        rank_with(candidates, |candidate| {
            vec![
                reason("temporal", 0.28 * candidate.signals.temporal),
                reason("novelty", 0.22 * candidate.signals.novelty),
                reason("exploration", 0.18 * candidate.signals.exploration),
                reason(
                    "reputation.creative_contribution",
                    0.12 * candidate.signals.reputation.creative_contribution,
                ),
                reason("social_distance", 0.10 * candidate.signals.social_distance),
                reason(
                    "source.emerging",
                    source_bonus(candidate, CandidateSource::Emerging, 0.10),
                ),
            ]
        })
    }
}

impl Lens for SlowInternetLens {
    fn id(&self) -> &'static str {
        "babble.lens.slow_internet.v1"
    }

    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        rank_with(candidates, |candidate| {
            vec![
                reason(
                    "evidence_quality",
                    0.26 * candidate.signals.evidence_quality,
                ),
                reason(
                    "reputation.research",
                    0.20 * candidate.signals.reputation.research_score(),
                ),
                reason("relevance", 0.18 * candidate.signals.relevance),
                reason("novelty", 0.14 * candidate.signals.novelty),
                reason(
                    "contradiction_context",
                    0.12 * candidate.signals.contradiction,
                ),
                reason("durability", 0.10 * (1.0 - candidate.signals.temporal)),
            ]
        })
    }
}

impl Lens for WeirdLens {
    fn id(&self) -> &'static str {
        "babble.lens.weird.v1"
    }

    fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        rank_with(candidates, |candidate| {
            vec![
                reason("novelty", 0.35 * candidate.signals.novelty),
                reason("exploration", 0.30 * candidate.signals.exploration),
                reason(
                    "semantic_distance",
                    0.20 * (1.0 - candidate.signals.relevance),
                ),
                reason(
                    "reputation.creative_contribution",
                    0.05 * candidate.signals.reputation.creative_contribution,
                ),
                reason(
                    "emerging",
                    source_bonus(candidate, CandidateSource::Emerging, 0.10),
                ),
                reason(
                    "contradiction",
                    source_bonus(candidate, CandidateSource::Contradiction, 0.05)
                        + 0.05 * candidate.signals.contradiction,
                ),
            ]
        })
    }
}

impl BuiltInLens {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Following,
            Self::Friends,
            Self::Research,
            Self::IntellectualSerendipity,
            Self::Contradictions,
            Self::Emerging,
            Self::SlowInternet,
            Self::Weird,
        ]
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Following => "babble.lens.following.v1",
            Self::Friends => "babble.lens.friends.v1",
            Self::Research => "babble.lens.research.v1",
            Self::IntellectualSerendipity => "babble.lens.intellectual_serendipity.v1",
            Self::Contradictions => "babble.lens.contradictions.v1",
            Self::Emerging => "babble.lens.emerging.v1",
            Self::SlowInternet => "babble.lens.slow_internet.v1",
            Self::Weird => "babble.lens.weird.v1",
        }
    }

    pub fn definition(&self) -> LensDefinition {
        let (name, description, required_signals, required_sources) = match self {
            Self::Following => (
                "Following",
                "Chronological following-oriented Lens for objects from followed identities and recent durable activity.",
                vec![
                    "followed_author",
                    "temporal",
                    "reputation.social_constructiveness",
                    "reputation.creative_contribution",
                    "relevance",
                ],
                vec![CandidateSource::Following, CandidateSource::SocialGraph],
            ),
            Self::Friends => (
                "Friends",
                "Close-social Lens that favors low social distance, followed authors, recent activity, and constructive reputation.",
                vec![
                    "social_distance",
                    "followed_author",
                    "temporal",
                    "reputation.social_constructiveness",
                    "relevance",
                ],
                vec![CandidateSource::Following, CandidateSource::SocialGraph],
            ),
            Self::Research => (
                "Research",
                "Evidence-heavy Lens for claims, sources, contradictions, and domain-quality signals.",
                vec![
                    "evidence_quality",
                    "relevance",
                    "contradiction",
                    "reputation.evidence_quality",
                    "reputation.domain_expertise",
                    "reputation.epistemic_accuracy",
                    "temporal",
                ],
                vec![CandidateSource::Evidence, CandidateSource::Contradiction],
            ),
            Self::IntellectualSerendipity => (
                "Intellectual Serendipity",
                "Adjacent-interest Lens that balances novelty, useful evidence, exploration, and nearby relevance.",
                vec![
                    "adjacent_relevance",
                    "novelty",
                    "evidence_quality",
                    "exploration",
                    "reputation.research",
                    "temporal",
                ],
                vec![
                    CandidateSource::SemanticNeighborhood,
                    CandidateSource::Exploration,
                    CandidateSource::Emerging,
                ],
            ),
            Self::Contradictions => (
                "Contradictions",
                "Context-seeking Lens that surfaces counterevidence, disagreement, and contradictory sources.",
                vec![
                    "contradiction",
                    "evidence.contradiction_score",
                    "evidence_quality",
                    "relevance",
                    "novelty",
                    "source.contradiction",
                ],
                vec![CandidateSource::Contradiction],
            ),
            Self::Emerging => (
                "Emerging",
                "New-activity Lens for fresh, novel, exploratory, and emerging-creator candidates.",
                vec![
                    "temporal",
                    "novelty",
                    "exploration",
                    "reputation.creative_contribution",
                    "social_distance",
                    "source.emerging",
                ],
                vec![CandidateSource::Emerging, CandidateSource::Exploration],
            ),
            Self::SlowInternet => (
                "Slow Internet",
                "Durable-context Lens that prefers evidence, relevance, reputation, and less time-sensitive objects.",
                vec![
                    "evidence_quality",
                    "reputation.research",
                    "relevance",
                    "novelty",
                    "contradiction_context",
                    "durability",
                ],
                vec![
                    CandidateSource::Evidence,
                    CandidateSource::SemanticNeighborhood,
                    CandidateSource::Contradiction,
                ],
            ),
            Self::Weird => (
                "Weird",
                "High-novelty Lens for distant, exploratory, creative, strange, and emerging objects.",
                vec![
                    "novelty",
                    "exploration",
                    "semantic_distance",
                    "reputation.creative_contribution",
                    "emerging",
                    "contradiction",
                ],
                vec![
                    CandidateSource::Exploration,
                    CandidateSource::Emerging,
                    CandidateSource::Contradiction,
                ],
            ),
        };
        LensDefinition {
            id: self.id().to_string(),
            lens: self.clone(),
            version: 1,
            name: name.to_string(),
            description: description.to_string(),
            execution: LensExecution::LocalDeterministic,
            required_signals: required_signals.into_iter().map(str::to_string).collect(),
            required_sources,
            required_permissions: Vec::new(),
        }
    }

    pub fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        match self {
            Self::Following => FollowingLens.rank(candidates),
            Self::Friends => FriendsLens.rank(candidates),
            Self::Research => ResearchLens.rank(candidates),
            Self::IntellectualSerendipity => IntellectualSerendipityLens.rank(candidates),
            Self::Contradictions => ContradictionsLens.rank(candidates),
            Self::Emerging => EmergingLens.rank(candidates),
            Self::SlowInternet => SlowInternetLens.rank(candidates),
            Self::Weird => WeirdLens.rank(candidates),
        }
    }
}

impl LensStack {
    pub fn new(id: impl Into<String>, weights: Vec<LensWeight>) -> Self {
        Self {
            id: id.into(),
            weights,
        }
    }

    pub fn rank(&self, candidates: &[Candidate]) -> Vec<RankedCandidate> {
        self.rank_with_trace(candidates).0
    }

    pub fn rank_with_trace(
        &self,
        candidates: &[Candidate],
    ) -> (Vec<RankedCandidate>, RankingTrace) {
        let mut merged: Vec<RankedCandidate> = candidates
            .iter()
            .cloned()
            .map(|candidate| RankedCandidate {
                candidate,
                score: 0.0,
                reasons: Vec::new(),
            })
            .collect();
        let mut contributions: Vec<(ObjectId, LensContribution)> = Vec::new();

        for lens_weight in normalized_weights(&self.weights) {
            for ranked in lens_weight.lens.rank(candidates) {
                if let Some(target) = merged
                    .iter_mut()
                    .find(|entry| entry.candidate.object_id == ranked.candidate.object_id)
                {
                    let weighted_score = ranked.score * lens_weight.weight;
                    target.score += weighted_score;
                    for reason in &ranked.reasons {
                        target.reasons.push(Reason {
                            signal: format!("{}:{}", lens_weight.lens.id(), reason.signal),
                            contribution: reason.contribution * lens_weight.weight,
                        });
                    }
                    contributions.push((
                        ranked.candidate.object_id.clone(),
                        LensContribution {
                            lens_id: lens_weight.lens.id().to_string(),
                            weight: lens_weight.weight,
                            score: weighted_score,
                            reasons: ranked
                                .reasons
                                .into_iter()
                                .map(|reason| Reason {
                                    signal: reason.signal,
                                    contribution: reason.contribution * lens_weight.weight,
                                })
                                .collect(),
                        },
                    ));
                }
            }
        }

        sort_ranked(&mut merged);
        let trace = RankingTrace {
            stack_id: self.id.clone(),
            candidates: merged
                .iter()
                .enumerate()
                .map(|(index, ranked)| CandidateTrace {
                    rank: index + 1,
                    object_id: ranked.candidate.object_id.clone(),
                    source: ranked.candidate.source.clone(),
                    sources: ranked.candidate.sources.clone(),
                    score: ranked.score,
                    lens_contributions: contributions
                        .iter()
                        .filter(|(object_id, _)| object_id == &ranked.candidate.object_id)
                        .map(|(_, contribution)| contribution.clone())
                        .collect(),
                })
                .collect(),
        };
        (merged, trace)
    }
}

impl Default for DiversityPolicy {
    fn default() -> Self {
        Self {
            max_source_share: 0.55,
            source_floors: vec![
                SourceFloor {
                    source: CandidateSource::Exploration,
                    minimum: 1,
                },
                SourceFloor {
                    source: CandidateSource::Contradiction,
                    minimum: 1,
                },
            ],
        }
    }
}

impl DiversityPolicy {
    pub fn bounded(&self) -> Self {
        Self {
            max_source_share: clamp(self.max_source_share),
            source_floors: self
                .source_floors
                .iter()
                .filter(|floor| floor.minimum > 0)
                .cloned()
                .collect(),
        }
    }
}

pub fn diversify_ranked(
    ranked: &[RankedCandidate],
    policy: &DiversityPolicy,
    limit: usize,
) -> (Vec<RankedCandidate>, DiversityTrace) {
    let policy = policy.bounded();
    let target_len = limit.min(ranked.len());
    let mut remaining = ranked.to_vec();
    let mut selected: Vec<RankedCandidate> = Vec::with_capacity(target_len);
    let mut trace_candidates = Vec::with_capacity(target_len);

    while !remaining.is_empty() && selected.len() < target_len {
        let best_index = best_diversity_candidate_index(&remaining, &selected, &policy);
        let mut candidate = remaining.remove(best_index);
        let lens_score = candidate.score;
        let reasons = diversity_reasons(&candidate, &selected, &policy);
        candidate.score = (lens_score
            + reasons
                .iter()
                .map(|reason| reason.contribution)
                .sum::<f64>())
        .clamp(0.0, 1.0);
        candidate
            .reasons
            .extend(reasons.iter().map(|reason| Reason {
                signal: format!("diversity:{}", reason.signal),
                contribution: reason.contribution,
            }));
        trace_candidates.push(DiversifiedCandidateTrace {
            rank: selected.len() + 1,
            object_id: candidate.candidate.object_id.clone(),
            source: candidate.candidate.source.clone(),
            lens_score,
            diversified_score: candidate.score,
            reasons,
        });
        selected.push(candidate);
    }

    let filtered = remaining
        .into_iter()
        .map(|candidate| candidate.candidate.object_id)
        .collect();
    (
        selected,
        DiversityTrace {
            policy,
            candidates: trace_candidates,
            filtered,
        },
    )
}

fn best_diversity_candidate_index(
    remaining: &[RankedCandidate],
    selected: &[RankedCandidate],
    policy: &DiversityPolicy,
) -> usize {
    let mut best_index = 0;
    for index in 1..remaining.len() {
        if diversity_ordering(&remaining[index], &remaining[best_index], selected, policy)
            == Ordering::Greater
        {
            best_index = index;
        }
    }
    best_index
}

fn diversity_ordering(
    left: &RankedCandidate,
    right: &RankedCandidate,
    selected: &[RankedCandidate],
    policy: &DiversityPolicy,
) -> Ordering {
    let left_score = diversity_adjusted_score(left, selected, policy);
    let right_score = diversity_adjusted_score(right, selected, policy);
    left_score
        .partial_cmp(&right_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| {
            left.score
                .partial_cmp(&right.score)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| left.candidate.created_at.cmp(&right.candidate.created_at))
        .then_with(|| right.candidate.object_id.cmp(&left.candidate.object_id))
}

fn diversity_adjusted_score(
    candidate: &RankedCandidate,
    selected: &[RankedCandidate],
    policy: &DiversityPolicy,
) -> f64 {
    (candidate.score
        + diversity_reasons(candidate, selected, policy)
            .iter()
            .map(|reason| reason.contribution)
            .sum::<f64>())
    .clamp(0.0, 1.0)
}

fn diversity_reasons(
    candidate: &RankedCandidate,
    selected: &[RankedCandidate],
    policy: &DiversityPolicy,
) -> Vec<DiversityReason> {
    let mut reasons = Vec::new();
    let floor_bonus = source_floor_bonus(candidate, selected, policy);
    if floor_bonus != 0.0 {
        reasons.push(diversity_reason("source_floor", floor_bonus));
    }
    let concentration_penalty = source_concentration_penalty(candidate, selected, policy);
    if concentration_penalty != 0.0 {
        reasons.push(diversity_reason(
            "source_concentration",
            -concentration_penalty,
        ));
    }
    reasons
}

fn source_floor_bonus(
    candidate: &RankedCandidate,
    selected: &[RankedCandidate],
    policy: &DiversityPolicy,
) -> f64 {
    if selected.is_empty() {
        return 0.0;
    }
    let mut bonus: f64 = 0.0;
    for floor in &policy.source_floors {
        if floor.source != candidate.candidate.source {
            continue;
        }
        let selected_count = selected
            .iter()
            .filter(|entry| entry.candidate.source == floor.source)
            .count();
        if selected_count < floor.minimum {
            bonus = bonus.max(0.18 / (selected_count + 1) as f64);
        }
    }
    bonus
}

fn source_concentration_penalty(
    candidate: &RankedCandidate,
    selected: &[RankedCandidate],
    policy: &DiversityPolicy,
) -> f64 {
    if selected.is_empty() {
        return 0.0;
    }
    let next_position = selected.len() + 1;
    let selected_count = selected
        .iter()
        .filter(|entry| entry.candidate.source == candidate.candidate.source)
        .count();
    let next_share = (selected_count + 1) as f64 / next_position as f64;
    if next_share <= policy.max_source_share {
        return 0.0;
    }
    ((next_share - policy.max_source_share) / (1.0 - policy.max_source_share).max(0.01) * 0.25)
        .clamp(0.0, 0.25)
}

fn diversity_reason(signal: &str, contribution: f64) -> DiversityReason {
    DiversityReason {
        signal: signal.to_string(),
        contribution,
    }
}

pub fn blend(lenses: &[(&dyn Lens, f64)], candidates: &[Candidate]) -> Vec<RankedCandidate> {
    let mut merged: Vec<RankedCandidate> = candidates
        .iter()
        .cloned()
        .map(|candidate| RankedCandidate {
            candidate,
            score: 0.0,
            reasons: Vec::new(),
        })
        .collect();

    for (lens, weight) in lenses {
        for ranked in lens.rank(candidates) {
            if let Some(target) = merged
                .iter_mut()
                .find(|entry| entry.candidate.object_id == ranked.candidate.object_id)
            {
                target.score += ranked.score * weight;
                for reason in ranked.reasons {
                    target.reasons.push(Reason {
                        signal: format!("{}:{}", lens.id(), reason.signal),
                        contribution: reason.contribution * weight,
                    });
                }
            }
        }
    }

    sort_ranked(&mut merged);
    merged
}

fn normalized_weights(weights: &[LensWeight]) -> Vec<LensWeight> {
    let mut sanitized: Vec<LensWeight> = weights
        .iter()
        .filter_map(|weight| {
            if weight.weight.is_finite() && weight.weight > 0.0 {
                Some(weight.clone())
            } else {
                None
            }
        })
        .collect();

    let total: f64 = sanitized.iter().map(|weight| weight.weight).sum();
    if total == 0.0 {
        sanitized.push(LensWeight {
            lens: BuiltInLens::Following,
            weight: 1.0,
        });
        return sanitized;
    }
    if total.is_finite() {
        for weight in &mut sanitized {
            weight.weight /= total;
        }
    } else {
        // Scaling only on overflow preserves ordinary v1 floating-point results.
        let maximum = sanitized
            .iter()
            .map(|weight| weight.weight)
            .fold(0.0, f64::max);
        let scaled_total: f64 = sanitized.iter().map(|weight| weight.weight / maximum).sum();
        for weight in &mut sanitized {
            weight.weight = (weight.weight / maximum) / scaled_total;
        }
    }
    sanitized
}

fn rank_with<F>(candidates: &[Candidate], explain: F) -> Vec<RankedCandidate>
where
    F: Fn(&Candidate) -> Vec<Reason>,
{
    let mut ranked: Vec<RankedCandidate> = candidates
        .iter()
        .cloned()
        .map(|candidate| {
            let reasons = explain(&candidate);
            let score = reasons.iter().map(|reason| reason.contribution).sum();
            RankedCandidate {
                candidate,
                score,
                reasons,
            }
        })
        .collect();
    sort_ranked(&mut ranked);
    ranked
}

fn sort_ranked(ranked: &mut [RankedCandidate]) {
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| right.candidate.created_at.cmp(&left.candidate.created_at))
            .then_with(|| left.candidate.object_id.cmp(&right.candidate.object_id))
    });
}

fn reason(signal: &str, contribution: f64) -> Reason {
    Reason {
        signal: signal.to_string(),
        contribution,
    }
}

fn source_bonus(candidate: &Candidate, expected: CandidateSource, value: f64) -> f64 {
    if candidate
        .sources
        .iter()
        .any(|source| source.source == expected)
    {
        value
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

fn positive(value: f64) -> f64 {
    if value.is_nan() {
        return 0.0;
    }
    value.max(0.0)
}
