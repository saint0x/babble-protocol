use babble_graph::{GraphIndex, Relation};
use babble_lens::{
    Candidate, CandidateSource, CandidateSourceContribution, EvidenceSignals, ReputationSignals,
    Signals,
};
use babble_types::{ObjectId, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod admission;
mod temporal;

pub use temporal::{
    NativeTemporalScorer, TemporalClass, TemporalEngagement, TemporalItem, TemporalProvider,
    TemporalProviderVersion, TemporalRequest, TemporalResult, TemporalScore,
};

pub const MAX_CANDIDATES: usize = 200;
pub const MAX_ANCHORS: usize = 64;
pub const MAX_FOLLOWED_OBJECTS: usize = 1_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectSignals {
    pub object_id: ObjectId,
    pub created_at: Timestamp,
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

impl ObjectSignals {
    pub fn candidate(&self, source: CandidateSource) -> Candidate {
        Candidate {
            object_id: self.object_id.clone(),
            source: source.clone(),
            sources: vec![CandidateSourceContribution {
                source,
                weight: 1.0,
            }],
            created_at: self.created_at,
            signals: Signals {
                social_distance: if self.followed_author { 0.0 } else { 0.7 },
                followed_author: self.followed_author,
                relevance: unit(self.relevance),
                novelty: unit(self.novelty),
                evidence_quality: unit(self.evidence_quality),
                contradiction: unit(self.contradiction),
                evidence: EvidenceSignals {
                    human_support: positive(self.evidence.human_support),
                    judgment_support: positive(self.evidence.judgment_support),
                    human_contradiction: positive(self.evidence.human_contradiction),
                    judgment_contradiction: positive(self.evidence.judgment_contradiction),
                },
                reputation: self.bounded_reputation(),
                temporal: unit(self.temporal),
                exploration: unit(self.exploration),
            },
        }
    }

    fn bounded_reputation(&self) -> ReputationSignals {
        ReputationSignals {
            epistemic_accuracy: unit(self.reputation.epistemic_accuracy),
            evidence_quality: unit(self.reputation.evidence_quality),
            social_constructiveness: unit(self.reputation.social_constructiveness),
            creative_contribution: unit(self.reputation.creative_contribution),
            moderation: unit(self.reputation.moderation),
            domain_expertise: unit(self.reputation.domain_expertise),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveryRequest {
    pub anchors: Vec<ObjectId>,
    /// Legacy public object follows only; never private person follows or history.
    pub followed_objects: BTreeSet<ObjectId>,
    /// Candidate budget for downstream rankers, capped at MAX_CANDIDATES.
    pub limit: usize,
    pub exploration_slots: usize,
}

impl DiscoveryRequest {
    pub fn for_anchors(anchors: Vec<ObjectId>) -> Self {
        Self {
            anchors,
            followed_objects: BTreeSet::new(),
            limit: 50,
            exploration_slots: 5,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CandidateEngine;

impl CandidateEngine {
    /// Refine provenance after bounded retrieval/enrichment. These formulas are
    /// identical to full-summary generation, but their scope is the admitted pool.
    pub fn annotate_admitted(
        &self,
        candidates: &mut [Candidate],
        summaries: &BTreeMap<ObjectId, ObjectSignals>,
        exploration_slots: usize,
    ) {
        let emerging: BTreeSet<_> = scored_candidates(summaries, 10, |summary| {
            unit(summary.temporal) * 0.55
                + unit(summary.novelty) * 0.25
                + summary.bounded_reputation().research_score() * 0.20
        })
        .into_iter()
        .collect();
        let exploration: BTreeSet<_> = scored_candidates(
            summaries,
            exploration_slots.min(MAX_CANDIDATES),
            |summary| {
                unit(summary.exploration) * 0.50
                    + unit(summary.novelty) * 0.35
                    + (1.0 - unit(summary.relevance)) * 0.15
            },
        )
        .into_iter()
        .collect();
        for candidate in candidates {
            for (source, members) in [
                (CandidateSource::Emerging, &emerging),
                (CandidateSource::Exploration, &exploration),
            ] {
                if members.contains(&candidate.object_id)
                    && !candidate.sources.iter().any(|s| s.source == source)
                {
                    candidate.sources.push(CandidateSourceContribution {
                        source,
                        weight: 1.0,
                    });
                }
            }
            candidate
                .sources
                .sort_by_key(|entry| std::cmp::Reverse(admission::source_priority(&entry.source)));
            candidate.source = candidate.sources[0].source.clone();
        }
    }

    pub fn generate(
        &self,
        graph: &GraphIndex,
        summaries: &BTreeMap<ObjectId, ObjectSignals>,
        request: &DiscoveryRequest,
    ) -> Vec<Candidate> {
        let limit = request.limit.min(MAX_CANDIDATES);
        if limit == 0 || summaries.is_empty() {
            return Vec::new();
        }
        let anchors = bounded_roots(&request.anchors, MAX_ANCHORS);
        let followed: BTreeSet<_> = request
            .followed_objects
            .iter()
            .take(MAX_FOLLOWED_OBJECTS)
            .collect();
        let seed = admission::request_seed(&anchors, &followed);
        let mut queues = admission::SourceQueues::new(summaries);
        queues.extend(CandidateSource::Following, followed.iter().copied());
        queues.extend(
            CandidateSource::SocialGraph,
            social_graph_candidates(graph, &followed),
        );
        queues.extend(CandidateSource::Temporal, anchors.iter().copied());
        queues.extend(
            CandidateSource::SocialGraph,
            social_graph_candidates(graph, &anchors),
        );
        for anchor in anchors {
            queues.extend(CandidateSource::Evidence, graph.supporting_evidence(anchor));
            queues.extend(
                CandidateSource::Contradiction,
                graph.contradicting_evidence(anchor),
            );
            queues.extend(
                CandidateSource::SemanticNeighborhood,
                semantic_neighbors(graph, anchor),
            );
        }
        queues.extend(
            CandidateSource::Emerging,
            scored_candidates(summaries, 10, |summary| {
                unit(summary.temporal) * 0.55
                    + unit(summary.novelty) * 0.25
                    + summary.bounded_reputation().research_score() * 0.20
            }),
        );
        queues.extend(
            CandidateSource::Exploration,
            scored_candidates(summaries, request.exploration_slots.min(limit), |summary| {
                unit(summary.exploration) * 0.50
                    + unit(summary.novelty) * 0.35
                    + (1.0 - unit(summary.relevance)) * 0.15
            }),
        );
        queues.select(limit, seed)
    }
}

fn bounded_roots(roots: &[ObjectId], limit: usize) -> BTreeSet<&ObjectId> {
    let mut bounded = BTreeSet::new();
    for root in roots {
        bounded.insert(root);
        if bounded.len() > limit {
            bounded.pop_last();
        }
    }
    bounded
}

fn social_graph_candidates<'a>(
    graph: &'a GraphIndex,
    roots: &BTreeSet<&ObjectId>,
) -> Vec<&'a ObjectId> {
    let mut neighbors = BTreeSet::new();
    for root in roots {
        neighbors.extend(graph.targets(root, &Relation::Follows));
        neighbors.extend(graph.sources(root, &Relation::Follows));
    }
    neighbors.into_iter().collect()
}

fn semantic_neighbors<'a>(graph: &'a GraphIndex, anchor: &ObjectId) -> Vec<&'a ObjectId> {
    let mut neighbors = Vec::new();
    for relation in [
        Relation::References,
        Relation::Cites,
        Relation::Quotes,
        Relation::Extends,
        Relation::DerivesFrom,
        Relation::Supersedes,
        Relation::Forks,
        Relation::Remixes,
    ] {
        neighbors.extend(graph.targets(anchor, &relation));
        neighbors.extend(graph.sources(anchor, &relation));
    }
    neighbors
}

// Retain only the best k references. Neither ranking nor retrieval clones candidates.
fn scored_candidates(
    summaries: &BTreeMap<ObjectId, ObjectSignals>,
    limit: usize,
    score: impl Fn(&ObjectSignals) -> f64,
) -> Vec<&ObjectId> {
    if limit == 0 {
        return Vec::new();
    }
    let mut best: Vec<(f64, &ObjectSignals)> = Vec::with_capacity(limit + 1);
    for (id, summary) in summaries {
        if id != &summary.object_id {
            continue;
        }
        let entry = (score(summary), summary);
        let position = best.partition_point(|current| {
            current
                .0
                .total_cmp(&entry.0)
                .reverse()
                .then_with(|| entry.1.created_at.cmp(&current.1.created_at))
                .then_with(|| current.1.object_id.cmp(&entry.1.object_id))
                .is_lt()
        });
        if position < limit {
            best.insert(position, entry);
            best.truncate(limit);
        }
    }
    best.into_iter()
        .map(|(_, summary)| &summary.object_id)
        .collect()
}

fn unit(value: f64) -> f64 {
    positive(value).min(1.0)
}

fn positive(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
