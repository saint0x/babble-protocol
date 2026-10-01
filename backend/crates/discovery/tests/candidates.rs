use babble_crypto::Keypair;
use babble_discovery::{CandidateEngine, DiscoveryRequest, ObjectSignals};
use babble_graph::{Edge, EdgeOrigin, GraphIndex, Relation};
use babble_identity::{Identity, IdentityKind};
use babble_lens::{
    CandidateSource, EvidenceSignals, Lens, ReputationSignals, ResearchLens, WeirdLens,
};
use babble_object::Object;
use babble_types::Timestamp;
use std::collections::{BTreeMap, BTreeSet};
use time::OffsetDateTime;

#[test]
fn discovery_builds_mixed_pool_from_graph_and_exploration() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "curator", &keypair).unwrap();

    let claim = object(&identity, &keypair, "claim");
    let support = object(&identity, &keypair, "support");
    let counter = object(&identity, &keypair, "counter");
    let adjacent = object(&identity, &keypair, "adjacent");
    let social = object(&identity, &keypair, "social");
    let odd = object(&identity, &keypair, "odd");

    let mut graph = GraphIndex::default();
    graph.insert(edge(
        &identity,
        &keypair,
        &support,
        &claim,
        Relation::EvidenceFor,
    ));
    graph.insert(edge(
        &identity,
        &keypair,
        &counter,
        &claim,
        Relation::EvidenceAgainst,
    ));
    graph.insert(edge(
        &identity,
        &keypair,
        &claim,
        &adjacent,
        Relation::References,
    ));
    graph.insert(edge(
        &identity,
        &keypair,
        &support,
        &social,
        Relation::Follows,
    ));

    let mut summaries = BTreeMap::new();
    summaries.insert(
        claim.id.clone(),
        signals(&claim, 10, false, 0.9, 0.2, 0.3, 0.2, 0.7, 0.8, 0.1),
    );
    summaries.insert(
        support.id.clone(),
        signals(&support, 20, false, 0.8, 0.1, 1.0, 0.1, 0.8, 0.6, 0.1),
    );
    summaries.insert(
        counter.id.clone(),
        signals(&counter, 30, false, 0.7, 0.2, 0.7, 1.0, 0.6, 0.5, 0.1),
    );
    summaries.insert(
        adjacent.id.clone(),
        signals(&adjacent, 40, false, 0.6, 0.5, 0.3, 0.2, 0.4, 0.4, 0.2),
    );
    summaries.insert(
        social.id.clone(),
        signals(&social, 45, false, 0.4, 0.4, 0.2, 0.1, 0.5, 0.4, 0.2),
    );
    summaries.insert(
        odd.id.clone(),
        signals(&odd, 50, false, 0.1, 1.0, 0.1, 0.1, 0.2, 0.2, 1.0),
    );

    let request = DiscoveryRequest {
        anchors: vec![claim.id.clone()],
        followed_objects: BTreeSet::from([support.id.clone()]),
        limit: 10,
        exploration_slots: 1,
    };
    let candidates = CandidateEngine.generate(&graph, &summaries, &request);

    assert!(candidates.iter().any(|candidate| {
        candidate.object_id == support.id
            && candidate
                .sources
                .iter()
                .any(|source| source.source == CandidateSource::Evidence)
    }));
    assert!(candidates.iter().any(|candidate| {
        candidate.object_id == claim.id
            && candidate
                .sources
                .iter()
                .any(|source| source.source == CandidateSource::Temporal)
    }));
    assert!(candidates.iter().any(|candidate| {
        candidate.object_id == adjacent.id
            && candidate
                .sources
                .iter()
                .any(|source| source.source == CandidateSource::SemanticNeighborhood)
    }));
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.source == CandidateSource::Contradiction)
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.source == CandidateSource::SemanticNeighborhood)
    );
    assert!(candidates.iter().any(|candidate| {
        candidate.object_id == social.id
            && candidate
                .sources
                .iter()
                .any(|source| source.source == CandidateSource::SocialGraph)
    }));
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.object_id == odd.id)
    );

    let research = ResearchLens.rank(&candidates);
    let weird = WeirdLens.rank(&candidates);
    assert_eq!(research[0].candidate.object_id, support.id);
    assert_eq!(weird[0].candidate.object_id, odd.id);
}

fn object(identity: &Identity, keypair: &Keypair, text: &str) -> Object {
    Object::text(identity, text)
        .unwrap()
        .sign(identity, keypair)
        .unwrap()
}

fn edge(
    identity: &Identity,
    keypair: &Keypair,
    source: &Object,
    target: &Object,
    relation: Relation,
) -> Edge {
    Edge::new(
        source.id.clone(),
        target.id.clone(),
        relation,
        EdgeOrigin::HumanAssertion,
        Some(identity.id.clone()),
    )
    .unwrap()
    .sign(identity, keypair)
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn signals(
    object: &Object,
    seconds: i64,
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
        object_id: object.id.clone(),
        created_at: Timestamp(OffsetDateTime::from_unix_timestamp(seconds).unwrap()),
        followed_author,
        relevance,
        novelty,
        evidence_quality,
        contradiction,
        evidence: EvidenceSignals {
            human_support: evidence_quality * 3.0,
            judgment_support: 0.0,
            human_contradiction: contradiction * 3.0,
            judgment_contradiction: 0.0,
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
