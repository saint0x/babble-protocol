use babel_discovery::{
    CandidateEngine, DiscoveryRequest, MAX_ANCHORS, MAX_CANDIDATES, MAX_FOLLOWED_OBJECTS,
    ObjectSignals,
};
use babel_graph::{Edge, EdgeOrigin, GraphIndex, Relation};
use babel_lens::{Candidate, CandidateSource, EvidenceSignals, ReputationSignals};
use babel_types::{EdgeId, ObjectId, Timestamp};
use std::collections::{BTreeMap, BTreeSet};
use time::OffsetDateTime;

fn id(index: usize) -> ObjectId {
    ObjectId::new_unchecked(format!("obj_{index:064x}"))
}

fn signals(index: usize) -> ObjectSignals {
    ObjectSignals {
        object_id: id(index),
        created_at: Timestamp(OffsetDateTime::UNIX_EPOCH),
        followed_author: false,
        relevance: 0.5,
        novelty: 0.0,
        evidence_quality: 0.0,
        contradiction: 0.0,
        evidence: EvidenceSignals::default(),
        reputation: ReputationSignals::default(),
        temporal: 0.0,
        exploration: 0.0,
    }
}

fn edge(index: usize, source: usize, target: usize, relation: Relation) -> Edge {
    Edge {
        id: EdgeId::new_unchecked(format!("edge_{index:064x}")),
        source: id(source),
        target: id(target),
        relation,
        origin: EdgeOrigin::HumanAssertion,
        author: None,
        created_at: Timestamp(OffsetDateTime::UNIX_EPOCH),
        metadata: BTreeMap::new(),
        signature: None,
    }
}

fn request(limit: usize, exploration_slots: usize) -> DiscoveryRequest {
    DiscoveryRequest {
        anchors: vec![id(0)],
        followed_objects: BTreeSet::new(),
        limit,
        exploration_slots,
    }
}

fn has(candidate: &Candidate, source: &CandidateSource) -> bool {
    candidate
        .sources
        .iter()
        .any(|entry| &entry.source == source)
}

fn count(candidates: &[Candidate], source: CandidateSource) -> usize {
    candidates
        .iter()
        .filter(|candidate| has(candidate, &source))
        .count()
}

fn saturated(
    reverse: bool,
) -> (
    GraphIndex,
    BTreeMap<ObjectId, ObjectSignals>,
    DiscoveryRequest,
) {
    let mut summaries = BTreeMap::new();
    let mut entries: Vec<_> = (0..1_271).map(signals).collect();
    for summary in &mut entries[1_251..1_271] {
        summary.exploration = 1.0;
        summary.novelty = 1.0;
    }
    for summary in &mut entries[1_241..1_251] {
        summary.temporal = 1.0;
    }
    let mut edges = Vec::new();
    for i in 251..501 {
        edges.push(edge(i, i, 0, Relation::EvidenceFor));
    }
    for i in 501..751 {
        edges.push(edge(i, i, 0, Relation::EvidenceAgainst));
    }
    for i in 751..1_001 {
        edges.push(edge(i, 0, i, Relation::Follows));
    }
    for i in 1_001..1_241 {
        edges.push(edge(i, i, 0, Relation::References));
    }
    if reverse {
        entries.reverse();
        edges.reverse();
    }
    for entry in entries {
        summaries.insert(entry.object_id.clone(), entry);
    }
    let mut graph = GraphIndex::default();
    for edge in edges {
        graph.insert(edge);
    }
    let mut request = request(40, 4);
    request.followed_objects = (1..251).map(id).collect();
    (graph, summaries, request)
}

#[test]
fn saturated_following_cannot_starve_later_sources() {
    let (graph, summaries, request) = saturated(false);
    let candidates = CandidateEngine.generate(&graph, &summaries, &request);
    assert_eq!(candidates.len(), 40);
    assert_eq!(count(&candidates, CandidateSource::Exploration), 4);
    for source in [
        CandidateSource::Following,
        CandidateSource::Evidence,
        CandidateSource::Contradiction,
        CandidateSource::SemanticNeighborhood,
        CandidateSource::Emerging,
        CandidateSource::SocialGraph,
    ] {
        assert!(count(&candidates, source) >= 5);
    }
    assert_eq!(count(&candidates, CandidateSource::Temporal), 1);
}

#[test]
fn selected_overlap_keeps_every_source_once_even_at_capacity() {
    let mut graph = GraphIndex::default();
    for (i, relation) in [
        Relation::EvidenceFor,
        Relation::EvidenceAgainst,
        Relation::References,
        Relation::Follows,
    ]
    .into_iter()
    .enumerate()
    {
        graph.insert(edge(i, 1, 0, relation.clone()));
        graph.insert(edge(i + 10, 1, 0, relation));
    }
    let mut request = request(1, 1);
    request.anchors.push(id(1));
    request.followed_objects.insert(id(1));
    let summary = signals(1);
    let summaries = BTreeMap::from([(id(1), summary.clone())]);
    let candidates = CandidateEngine.generate(&graph, &summaries, &request);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].source, CandidateSource::Following);
    assert_eq!(candidates[0].sources.len(), 8);
    let distinct: BTreeSet<_> = candidates[0]
        .sources
        .iter()
        .map(|entry| &entry.source)
        .collect();
    assert_eq!(distinct.len(), 8);
    assert!(
        candidates[0]
            .sources
            .iter()
            .all(|entry| entry.weight == 1.0)
    );
    assert_eq!(
        candidates[0].signals,
        summary.candidate(CandidateSource::Following).signals
    );
    assert_eq!(candidates[0].signals.evidence_quality, 0.0);
    assert_eq!(candidates[0].signals.contradiction, 0.0);
}

#[test]
fn missing_and_mismatched_ids_do_not_consume_slots_or_fabricate_objects() {
    let mut graph = GraphIndex::default();
    graph.insert(edge(1, 90, 0, Relation::EvidenceFor));
    graph.insert(edge(2, 91, 0, Relation::EvidenceAgainst));
    let summaries = BTreeMap::from([(id(1), signals(1)), (id(2), signals(1))]);
    let mut request = request(200, 200);
    request.followed_objects = (0..100).map(id).collect();
    let candidates = CandidateEngine.generate(&graph, &summaries, &request);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].object_id, id(1));
    assert!(!has(&candidates[0], &CandidateSource::Evidence));
    assert!(!has(&candidates[0], &CandidateSource::Contradiction));
    assert!(!has(&candidates[0], &CandidateSource::Temporal));
}

#[test]
fn zero_limit_and_empty_inputs_return_no_candidates() {
    let (graph, summaries, mut request) = saturated(false);
    request.limit = 0;
    request.exploration_slots = usize::MAX;
    assert!(
        CandidateEngine
            .generate(&graph, &summaries, &request)
            .is_empty()
    );
    request.limit = usize::MAX;
    assert!(
        CandidateEngine
            .generate(&graph, &BTreeMap::new(), &request)
            .is_empty()
    );
}

#[test]
fn candidate_budget_caps_at_200_without_overflow_and_feeds_a_rich_pool() {
    let (graph, summaries, mut request) = saturated(false);
    for limit in [1, 2, 8, 50, 200, 201, usize::MAX] {
        request.limit = limit;
        for slots in [0, 1, 5, 200, usize::MAX] {
            request.exploration_slots = slots;
            let candidates = CandidateEngine.generate(&graph, &summaries, &request);
            let cap = limit.min(MAX_CANDIDATES);
            assert_eq!(candidates.len(), cap);
            assert_eq!(
                count(&candidates, CandidateSource::Exploration),
                slots.min(cap)
            );
            let unique: BTreeSet<_> = candidates
                .iter()
                .map(|candidate| &candidate.object_id)
                .collect();
            assert_eq!(unique.len(), cap);
            assert!(
                candidates
                    .iter()
                    .all(|candidate| !candidate.sources.is_empty())
            );
        }
    }
}

#[test]
fn insertion_order_and_anchor_duplicates_do_not_change_selection_or_provenance() {
    let (graph, summaries, mut request) = saturated(false);
    let (reverse_graph, reverse_summaries, mut reverse_request) = saturated(true);
    request.anchors.extend([id(2), id(3)]);
    reverse_request.anchors = vec![id(3), id(0), id(2), id(3), id(0)];
    for limit in 1..=20 {
        request.limit = limit;
        reverse_request.limit = limit;
        assert_eq!(
            CandidateEngine.generate(&graph, &summaries, &request),
            CandidateEngine.generate(&reverse_graph, &reverse_summaries, &reverse_request),
        );
    }
}

#[test]
fn small_capacities_are_repeatable_and_reservations_take_precedence() {
    let (graph, summaries, mut request) = saturated(false);
    for slots in [0, 1, 3] {
        request.exploration_slots = slots;
        for limit in 1..8 {
            request.limit = limit;
            let first = CandidateEngine.generate(&graph, &summaries, &request);
            assert_eq!(
                first,
                CandidateEngine.generate(&graph, &summaries, &request)
            );
            assert_eq!(first.len(), limit);
            assert_eq!(
                count(&first, CandidateSource::Exploration),
                slots.min(limit)
            );
        }
    }
}

#[test]
fn exhausted_and_overlapping_queues_refill_from_remaining_unique_members() {
    let mut graph = GraphIndex::default();
    let summaries: BTreeMap<_, _> = (1..301).map(|i| (id(i), signals(i))).collect();
    for i in 1..301 {
        graph.insert(edge(i, i, 0, Relation::EvidenceFor));
        graph.insert(edge(i + 300, i, 0, Relation::EvidenceAgainst));
    }
    let mut request = request(200, 0);
    request.followed_objects.insert(id(1));
    let candidates = CandidateEngine.generate(&graph, &summaries, &request);
    assert_eq!(candidates.len(), 200);
    for candidate in &candidates {
        assert!(has(candidate, &CandidateSource::Evidence));
        assert!(has(candidate, &CandidateSource::Contradiction));
        assert!(!has(candidate, &CandidateSource::Exploration));
    }
}

#[test]
fn bounded_public_roots_are_canonical_and_excess_roots_do_not_leak_membership() {
    let summaries: BTreeMap<_, _> = (0..MAX_FOLLOWED_OBJECTS + 20)
        .map(|i| (id(i), signals(i)))
        .collect();
    let mut request = request(200, 200);
    request.anchors = (0..MAX_ANCHORS + 20).rev().map(id).collect();
    request.followed_objects = summaries.keys().cloned().collect();
    let graph = GraphIndex::default();
    let candidates = CandidateEngine.generate(&graph, &summaries, &request);
    let mut bounded = request.clone();
    bounded.anchors = (0..MAX_ANCHORS).map(id).collect();
    bounded.followed_objects = (0..MAX_FOLLOWED_OBJECTS).map(id).collect();
    assert_eq!(
        candidates,
        CandidateEngine.generate(&graph, &summaries, &bounded)
    );
    assert_eq!(count(&candidates, CandidateSource::Temporal), MAX_ANCHORS);
    // Select an excess followed root through exploration; it must not acquire Following provenance.
    let mut summaries = summaries;
    summaries
        .get_mut(&id(MAX_FOLLOWED_OBJECTS + 1))
        .unwrap()
        .exploration = 1.0;
    let candidates = CandidateEngine.generate(&graph, &summaries, &request);
    let excess = candidates
        .iter()
        .find(|candidate| candidate.object_id == id(MAX_FOLLOWED_OBJECTS + 1))
        .unwrap();
    assert!(!has(excess, &CandidateSource::Following));
}

#[test]
fn nonfinite_and_out_of_range_signals_are_normalized_without_source_boosts() {
    let mut summary = signals(1);
    summary.relevance = f64::NAN;
    summary.novelty = f64::INFINITY;
    summary.temporal = f64::NEG_INFINITY;
    summary.exploration = -4.0;
    summary.evidence_quality = f64::NAN;
    summary.contradiction = 9.0;
    summary.evidence = EvidenceSignals {
        human_support: f64::NAN,
        judgment_support: f64::INFINITY,
        human_contradiction: -1.0,
        judgment_contradiction: 4.0,
    };
    summary.reputation = ReputationSignals {
        epistemic_accuracy: f64::NAN,
        evidence_quality: f64::INFINITY,
        social_constructiveness: -3.0,
        creative_contribution: 3.0,
        moderation: f64::NEG_INFINITY,
        domain_expertise: 0.5,
    };
    let summaries = BTreeMap::from([(id(1), summary)]);
    let candidates = CandidateEngine.generate(&GraphIndex::default(), &summaries, &request(1, 1));
    let signals = &candidates[0].signals;
    assert_eq!(
        (
            signals.relevance,
            signals.novelty,
            signals.temporal,
            signals.exploration
        ),
        (0.0, 0.0, 0.0, 0.0)
    );
    assert_eq!(
        (signals.evidence_quality, signals.contradiction),
        (0.0, 1.0)
    );
    assert_eq!(
        signals.evidence,
        EvidenceSignals {
            judgment_contradiction: 4.0,
            ..EvidenceSignals::default()
        }
    );
    assert_eq!(
        signals.reputation,
        ReputationSignals {
            creative_contribution: 1.0,
            domain_expertise: 0.5,
            ..ReputationSignals::default()
        }
    );
}

#[test]
fn bounded_scoring_and_ties_choose_actual_top_exploration_members() {
    let mut summaries: BTreeMap<_, _> = (0..30).map(|i| (id(i), signals(i))).collect();
    summaries.get_mut(&id(29)).unwrap().exploration = 1.0;
    summaries.get_mut(&id(28)).unwrap().exploration = f64::NAN;
    summaries.get_mut(&id(27)).unwrap().created_at =
        Timestamp(OffsetDateTime::from_unix_timestamp(1).unwrap());
    let candidates = CandidateEngine.generate(&GraphIndex::default(), &summaries, &request(2, 2));
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.object_id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([id(27), id(29)])
    );
    assert_eq!(count(&candidates, CandidateSource::Exploration), 2);
}

#[test]
fn retrieval_keeps_existing_relation_directions_and_excludes_unrelated_relations() {
    let mut graph = GraphIndex::default();
    for (i, relation) in [
        Relation::References,
        Relation::Cites,
        Relation::Quotes,
        Relation::Extends,
        Relation::DerivesFrom,
        Relation::Supersedes,
        Relation::Forks,
        Relation::Remixes,
        Relation::Follows,
    ]
    .into_iter()
    .enumerate()
    {
        graph.insert(edge(i, 0, i + 1, relation.clone()));
        graph.insert(edge(i + 20, i + 11, 0, relation));
    }
    graph.insert(edge(50, 0, 30, Relation::EvidenceFor));
    graph.insert(edge(51, 0, 31, Relation::EvidenceAgainst));
    graph.insert(edge(52, 32, 0, Relation::Supports));
    graph.insert(edge(53, 33, 0, Relation::Contradicts));
    let summaries: BTreeMap<_, _> = (0..40).map(|i| (id(i), signals(i))).collect();
    let candidates = CandidateEngine.generate(&graph, &summaries, &request(200, 200));
    assert_eq!(
        count(&candidates, CandidateSource::SemanticNeighborhood),
        16
    );
    assert_eq!(count(&candidates, CandidateSource::SocialGraph), 2);
    assert_eq!(count(&candidates, CandidateSource::Evidence), 0);
    assert_eq!(count(&candidates, CandidateSource::Contradiction), 0);
}
