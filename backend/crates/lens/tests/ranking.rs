use babel_lens::{
    BuiltInLens, Candidate, CandidateSource, CandidateSourceContribution, ContradictionsLens,
    DiversityPolicy, EmergingLens, EvidenceSignals, FollowingLens, FriendsLens,
    IntellectualSerendipityLens, Lens, LensExecution, LensStack, LensWeight, ReputationSignals,
    ResearchLens, Signals, SlowInternetLens, SourceFloor, WeirdLens, diversify_ranked,
};
use babel_types::{Hash, ObjectId, Timestamp};
use time::OffsetDateTime;

#[test]
fn built_in_lenses_rank_same_pool_differently() {
    let candidates = vec![
        candidate(
            "a",
            CandidateSource::Following,
            10,
            signals(true, 0.70, 0.20, 0.30, 0.90, 0.90, 0.95, 0.10),
        ),
        candidate(
            "b",
            CandidateSource::Evidence,
            20,
            signals(false, 0.95, 0.10, 0.98, 0.20, 0.80, 0.40, 0.05),
        ),
        candidate(
            "c",
            CandidateSource::Exploration,
            30,
            signals(false, 0.20, 0.99, 0.10, 0.85, 0.20, 0.50, 1.00),
        ),
    ];

    let following = FollowingLens.rank(&candidates);
    let friends = FriendsLens.rank(&candidates);
    let research = ResearchLens.rank(&candidates);
    let contradictions = ContradictionsLens.rank(&candidates);
    let emerging = EmergingLens.rank(&candidates);
    let weird = WeirdLens.rank(&candidates);

    assert_eq!(following[0].candidate.object_id, candidates[0].object_id);
    assert_eq!(friends[0].candidate.object_id, candidates[0].object_id);
    assert_eq!(research[0].candidate.object_id, candidates[1].object_id);
    assert_eq!(
        contradictions[0].candidate.object_id,
        candidates[0].object_id
    );
    assert_eq!(emerging[0].candidate.object_id, candidates[2].object_id);
    assert_eq!(weird[0].candidate.object_id, candidates[2].object_id);
    assert_ne!(following[0].reasons, research[0].reasons);
}

#[test]
fn built_in_lenses_expose_inspectable_definitions() {
    let definitions = BuiltInLens::all()
        .into_iter()
        .map(|lens| lens.definition())
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 8);
    assert!(definitions.iter().all(|definition| {
        definition.id.starts_with("babel.lens.")
            && definition.version == 1
            && definition.execution == LensExecution::LocalDeterministic
            && !definition.required_signals.is_empty()
            && definition.required_permissions.is_empty()
    }));
    let research = definitions
        .iter()
        .find(|definition| definition.lens == BuiltInLens::Research)
        .expect("Research Lens should be cataloged");
    assert!(
        research
            .required_signals
            .iter()
            .any(|signal| signal == "evidence_quality")
    );
    assert!(
        research
            .required_sources
            .contains(&CandidateSource::Evidence)
    );
}

#[test]
fn spec_lenses_expose_distinct_policy_traces() {
    let candidates = vec![
        candidate(
            "durable",
            CandidateSource::Evidence,
            10,
            signals(false, 0.82, 0.60, 0.95, 0.65, 0.95, 0.05, 0.20),
        ),
        candidate(
            "adjacent",
            CandidateSource::SemanticNeighborhood,
            20,
            signals(false, 0.62, 0.90, 0.70, 0.20, 0.60, 0.55, 0.75),
        ),
    ];

    let serendipity = IntellectualSerendipityLens.rank(&candidates);
    let slow = SlowInternetLens.rank(&candidates);

    assert_eq!(serendipity[0].candidate.object_id, candidates[1].object_id);
    assert_eq!(slow[0].candidate.object_id, candidates[0].object_id);
    assert!(
        serendipity[0]
            .reasons
            .iter()
            .any(|reason| reason.signal == "adjacent_relevance")
    );
    assert!(
        slow[0]
            .reasons
            .iter()
            .any(|reason| reason.signal == "durability")
    );
}

#[test]
fn signal_bounds_remove_nan_and_out_of_range_values() {
    let bounded = Signals {
        social_distance: f64::NAN,
        followed_author: false,
        relevance: 2.0,
        novelty: -1.0,
        evidence_quality: 0.5,
        contradiction: 0.4,
        evidence: EvidenceSignals {
            human_support: -1.0,
            judgment_support: f64::NAN,
            human_contradiction: 2.0,
            judgment_contradiction: 0.5,
        },
        reputation: ReputationSignals {
            epistemic_accuracy: 2.0,
            evidence_quality: -1.0,
            social_constructiveness: 0.3,
            creative_contribution: 0.2,
            moderation: f64::NAN,
            domain_expertise: 0.4,
        },
        temporal: 0.2,
        exploration: 0.1,
    }
    .bounded();

    assert_eq!(bounded.social_distance, 0.0);
    assert_eq!(bounded.relevance, 1.0);
    assert_eq!(bounded.novelty, 0.0);
    assert_eq!(bounded.evidence.human_support, 0.0);
    assert_eq!(bounded.evidence.judgment_support, 0.0);
    assert_eq!(bounded.evidence.human_contradiction, 2.0);
    assert_eq!(bounded.reputation.epistemic_accuracy, 1.0);
    assert_eq!(bounded.reputation.evidence_quality, 0.0);
    assert_eq!(bounded.reputation.moderation, 0.0);
}

#[test]
fn lens_stack_returns_weighted_ranking_trace() {
    let candidates = vec![
        candidate(
            "a",
            CandidateSource::Following,
            10,
            signals(true, 0.80, 0.10, 0.30, 0.20, 0.80, 0.90, 0.10),
        ),
        candidate(
            "b",
            CandidateSource::Evidence,
            20,
            signals(false, 0.95, 0.20, 1.00, 0.20, 0.90, 0.50, 0.10),
        ),
        candidate(
            "c",
            CandidateSource::Exploration,
            30,
            signals(false, 0.10, 1.00, 0.10, 0.60, 0.20, 0.20, 1.00),
        ),
    ];
    let stack = LensStack::new(
        "balanced-test",
        vec![
            LensWeight {
                lens: BuiltInLens::IntellectualSerendipity,
                weight: 2.0,
            },
            LensWeight {
                lens: BuiltInLens::Research,
                weight: 3.0,
            },
            LensWeight {
                lens: BuiltInLens::Weird,
                weight: 1.0,
            },
            LensWeight {
                lens: BuiltInLens::Following,
                weight: f64::NAN,
            },
        ],
    );

    let (ranked, trace) = stack.rank_with_trace(&candidates);

    assert_eq!(ranked.len(), candidates.len());
    assert_eq!(trace.stack_id, "balanced-test");
    assert_eq!(trace.candidates[0].rank, 1);
    assert_eq!(trace.candidates[0].object_id, ranked[0].candidate.object_id);
    assert_eq!(trace.candidates[0].lens_contributions.len(), 3);
    assert!(
        trace.candidates[0]
            .lens_contributions
            .iter()
            .all(|contribution| contribution.weight > 0.0)
    );
    assert!(trace.candidates.iter().any(|candidate| {
        candidate
            .lens_contributions
            .iter()
            .any(|contribution| contribution.lens_id == "babel.lens.research.v1")
    }));
}

#[test]
fn empty_lens_stack_falls_back_to_following() {
    let candidates = vec![candidate(
        "a",
        CandidateSource::Following,
        10,
        signals(true, 0.80, 0.10, 0.30, 0.20, 0.80, 0.90, 0.10),
    )];
    let stack = LensStack::new("empty", Vec::new());

    let (_, trace) = stack.rank_with_trace(&candidates);

    assert_eq!(
        trace.candidates[0].lens_contributions[0].lens_id,
        "babel.lens.following.v1"
    );
    assert_eq!(trace.candidates[0].lens_contributions[0].weight, 1.0);
}

#[test]
fn diversity_reranker_preserves_source_floors_after_lens_ranking() {
    let ranked = ResearchLens.rank(&[
        candidate(
            "follow-a",
            CandidateSource::Following,
            10,
            signals(true, 0.98, 0.80, 1.00, 0.45, 0.95, 0.85, 0.10),
        ),
        candidate(
            "follow-b",
            CandidateSource::Following,
            20,
            signals(true, 0.94, 0.30, 0.84, 0.10, 0.90, 0.80, 0.10),
        ),
        candidate(
            "counter",
            CandidateSource::Contradiction,
            30,
            signals(false, 0.78, 0.72, 0.80, 0.85, 0.80, 0.60, 0.10),
        ),
        candidate(
            "explore",
            CandidateSource::Exploration,
            40,
            signals(false, 0.80, 0.95, 0.90, 0.40, 0.70, 0.50, 1.00),
        ),
    ]);

    let (diversified, trace) = diversify_ranked(&ranked, &DiversityPolicy::default(), 3);

    assert_eq!(diversified[0].candidate.source, CandidateSource::Following);
    assert!(
        diversified
            .iter()
            .any(|candidate| candidate.candidate.source == CandidateSource::Contradiction)
    );
    assert!(
        diversified
            .iter()
            .any(|candidate| candidate.candidate.source == CandidateSource::Exploration)
    );
    assert!(trace.candidates.iter().any(|candidate| {
        candidate
            .reasons
            .iter()
            .any(|reason| reason.signal == "source_floor" && reason.contribution > 0.0)
    }));
    assert_eq!(trace.filtered.len(), 1);
}

#[test]
fn diversity_reranker_penalizes_single_source_concentration() {
    let ranked = WeirdLens.rank(&[
        candidate(
            "explore-a",
            CandidateSource::Exploration,
            10,
            signals(false, 0.10, 0.98, 0.30, 0.20, 0.40, 0.70, 1.00),
        ),
        candidate(
            "explore-b",
            CandidateSource::Exploration,
            20,
            signals(false, 0.10, 0.97, 0.30, 0.20, 0.40, 0.70, 0.98),
        ),
        candidate(
            "semantic",
            CandidateSource::SemanticNeighborhood,
            30,
            signals(false, 0.30, 0.90, 0.50, 0.10, 0.40, 0.70, 0.88),
        ),
    ]);
    let policy = DiversityPolicy {
        max_source_share: 0.50,
        source_floors: vec![SourceFloor {
            source: CandidateSource::SemanticNeighborhood,
            minimum: 1,
        }],
    };

    let (diversified, trace) = diversify_ranked(&ranked, &policy, 3);

    assert_eq!(
        diversified[0].candidate.source,
        CandidateSource::Exploration
    );
    assert_eq!(
        diversified[1].candidate.source,
        CandidateSource::SemanticNeighborhood
    );
    assert!(
        trace.candidates[2]
            .reasons
            .iter()
            .any(|reason| reason.signal == "source_concentration" && reason.contribution < 0.0)
    );
}

fn candidate(suffix: &str, source: CandidateSource, seconds: i64, signals: Signals) -> Candidate {
    let hash = Hash::from_bytes(suffix.as_bytes());
    Candidate {
        object_id: ObjectId::from_hash(&hash),
        source: source.clone(),
        sources: vec![CandidateSourceContribution {
            source,
            weight: 1.0,
        }],
        created_at: Timestamp(OffsetDateTime::from_unix_timestamp(seconds).unwrap()),
        signals,
    }
}

#[allow(clippy::too_many_arguments)]
fn signals(
    followed_author: bool,
    relevance: f64,
    novelty: f64,
    evidence_quality: f64,
    contradiction: f64,
    reputation: f64,
    temporal: f64,
    exploration: f64,
) -> Signals {
    Signals {
        social_distance: 0.5,
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
    .bounded()
}
