from babble_algorithms import (
    CandidateEngine,
    CommunityModerator,
    ConsensusAnalyzer,
    ConsensusSource,
    ConsensusState,
    ContentProfile,
    ContentTimeClass,
    EngagementAnalyzer,
    EngagementEvent,
    EngagementWindow,
    FeedDiversifier,
    FeedObjectContext,
    Interaction,
    LensStack,
    LocalJudgmentProvider,
    ModerationContext,
    RecommendationEngine,
    TemporalInput,
    TemporalScorer,
    UserProfile,
)
from babble_algorithms.discovery import DiscoveryRequest
from babble_algorithms.lens import BuiltInLens, LensWeight
from babble_algorithms.types import (
    Candidate,
    CandidateSource,
    ObjectId,
    ObjectSignals,
    RankedCandidate,
)


def test_local_judgment_is_deterministic() -> None:
    provider = LocalJudgmentProvider()

    first = provider.judge(
        "babble.judgment.evidence_quality.v1",
        "According to the dataset, replication confirms the measurement.",
    )
    second = provider.judge(
        "babble.judgment.evidence_quality.v1",
        "According to the dataset, replication confirms the measurement.",
    )

    assert first == second
    assert first.score > 0.6


def test_candidate_engine_mixes_sources_without_duplicates() -> None:
    shared = ObjectId("obj:shared")
    request = DiscoveryRequest(
        followed=(shared,),
        contradicting_evidence=(shared, ObjectId("obj:contradiction")),
        semantic_neighbors=(ObjectId("obj:neighbor"),),
        object_signals={
            shared: ObjectSignals(relevance=0.9),
            ObjectId("obj:extra"): ObjectSignals(weirdness=1.0),
        },
        exploration_slots=1,
    )

    candidates = CandidateEngine().candidates(request)

    assert [candidate.object_id for candidate in candidates] == [
        ObjectId("obj:shared"),
        ObjectId("obj:contradiction"),
        ObjectId("obj:neighbor"),
        ObjectId("obj:extra"),
    ]
    assert candidates[0].source == "Following"
    assert candidates[-1].source == "Exploration"


def test_lens_stack_returns_explainable_ranking() -> None:
    candidates = CandidateEngine().candidates(
        DiscoveryRequest(
            contradicting_evidence=(ObjectId("obj:research"),),
            emerging=(ObjectId("obj:weird"),),
            object_signals={
                ObjectId("obj:research"): ObjectSignals(
                    relevance=0.7, novelty=0.72, evidence_quality=1.0, contradiction=1.0
                ),
                ObjectId("obj:weird"): ObjectSignals(weirdness=1.0, novelty=1.0, emerging=1.0),
            },
        )
    )
    stack = LensStack(
        (
            LensWeight(BuiltInLens.RESEARCH, 0.7),
            LensWeight(BuiltInLens.WEIRD, 0.3),
        )
    )

    trace = stack.rank(candidates)

    assert trace.ranked[0].candidate.object_id == ObjectId("obj:research")
    assert trace.ranked[0].contributions[0].lens == "research"


def test_spec_lenses_are_available_and_policy_distinct() -> None:
    candidates = CandidateEngine().candidates(
        DiscoveryRequest(
            contradicting_evidence=(ObjectId("obj:counter"),),
            semantic_neighbors=(ObjectId("obj:adjacent"),),
            emerging=(ObjectId("obj:emerging"),),
            object_signals={
                ObjectId("obj:counter"): ObjectSignals(
                    relevance=0.8,
                    evidence_quality=0.85,
                    contradiction=1.0,
                    recency=0.2,
                ),
                ObjectId("obj:adjacent"): ObjectSignals(
                    relevance=0.62,
                    novelty=0.95,
                    evidence_quality=0.65,
                    weirdness=0.75,
                ),
                ObjectId("obj:emerging"): ObjectSignals(
                    novelty=0.85,
                    emerging=1.0,
                    recency=1.0,
                    weirdness=0.8,
                ),
            },
        )
    )

    serendipity = LensStack(
        (LensWeight(BuiltInLens.INTELLECTUAL_SERENDIPITY, 1.0),)
    ).rank(candidates)
    contradictions = LensStack((LensWeight(BuiltInLens.CONTRADICTIONS, 1.0),)).rank(
        candidates
    )
    emerging = LensStack((LensWeight(BuiltInLens.EMERGING, 1.0),)).rank(candidates)
    slow = LensStack((LensWeight(BuiltInLens.SLOW_INTERNET, 1.0),)).rank(candidates)

    assert serendipity.ranked[0].candidate.object_id == ObjectId("obj:adjacent")
    assert contradictions.ranked[0].candidate.object_id == ObjectId("obj:counter")
    assert emerging.ranked[0].candidate.object_id == ObjectId("obj:emerging")
    assert slow.ranked[0].candidate.object_id == ObjectId("obj:counter")


def test_community_moderator_flags_spam_from_real_signals() -> None:
    result = CommunityModerator().analyze(
        "obj:spam",
        "BUY NOW!!! Limited time special offer, click here https://example.invalid",
        context=ModerationContext(
            repeated_messages=4, account_age_days=0.5, similar_recent_posts=5
        ),
    )

    assert result.action in {"limit", "flag", "remove"}
    assert "spam" in result.flags
    assert result.scores.spam >= 0.7
    assert result.scores.quality < 0.8


def test_temporal_scorer_combines_recency_decay_and_velocity() -> None:
    scorer = TemporalScorer()
    fresh = scorer.score(
        TemporalInput(
            content_id="obj:fresh",
            published_at=1_000_000.0,
            content_class=ContentTimeClass.NEWS,
            engagement=EngagementWindow(
                total_views=100,
                recent_views=80,
                total_interactions=20,
                recent_interactions=14,
            ),
            quality_score=0.9,
            tags=("breaking",),
        ),
        reference_time=1_000_000.0 + 3_600.0,
    )
    old = scorer.score(
        TemporalInput(
            content_id="obj:old",
            published_at=1_000_000.0 - 40 * 24 * 3_600.0,
            content_class=ContentTimeClass.REFERENCE,
            quality_score=0.9,
            tags=("evergreen",),
        ),
        reference_time=1_000_000.0,
    )

    assert fresh.recency > old.recency
    assert fresh.engagement_velocity > old.engagement_velocity
    assert fresh.survival_score > old.survival_score


def test_engagement_analyzer_summarizes_segments_and_content_performance() -> None:
    summary = EngagementAnalyzer().summarize(
        (
            EngagementEvent("user:1", "obj:a", 10_000.0, 280.0, 0.9, "surface_open"),
            EngagementEvent("user:1", "obj:a", 10_300.0, 220.0, 0.8, "reply"),
            EngagementEvent("user:2", "obj:b", 10_600.0, 20.0, 0.1, "view"),
        ),
        reference_time=11_000.0,
        window_seconds=3_600.0,
    )

    assert summary.total_sessions == 3
    assert summary.user_segments["highly_engaged"] == 1
    assert summary.user_segments["low_engagement"] == 1
    assert summary.content_performance["obj:a"].interaction_rate == 1.0
    assert summary.content_performance["obj:a"].engagement_score > summary.content_performance[
        "obj:b"
    ].engagement_score


def test_consensus_establishes_and_can_revoke_previous_consensus() -> None:
    analyzer = ConsensusAnalyzer()
    strong_sources = (
        ConsensusSource(
            "source:1",
            "research_paper",
            "The protocol has deterministic canonical encoding. It has stable signed object IDs.",
            900_000.0,
            quality_score=0.95,
            evidence_score=0.95,
            user_id="user:1",
            vote=0.95,
        ),
        ConsensusSource(
            "source:2",
            "official_docs",
            "The protocol has deterministic canonical encoding. It has stable signed object IDs.",
            900_500.0,
            quality_score=0.96,
            evidence_score=0.94,
            user_id="user:2",
            vote=0.92,
        ),
    )
    established = analyzer.evaluate(
        "obj:claim", strong_sources, reference_time=901_000.0
    )
    weak = analyzer.evaluate(
        "obj:claim",
        (
            ConsensusSource(
                "source:3",
                "social_media",
                "Maybe unrelated speculation with no evidence.",
                901_000.0,
                quality_score=0.1,
                evidence_score=0.0,
                vote=0.05,
            ),
        ),
        reference_time=901_100.0,
        previous_score=0.9,
    )

    assert established.state == ConsensusState.ESTABLISHED
    assert weak.state == ConsensusState.REVOKED


def test_recommendation_engine_ranks_from_real_vectors_and_signals() -> None:
    engine = RecommendationEngine()
    user = UserProfile(
        "user:main",
        interests=("protocol", "runtime", "interactive surfaces"),
        expertise=("distributed systems",),
        history=(Interaction("obj:surface", 0.9, 1_000.0),),
    )
    peer = UserProfile(
        "user:peer",
        interests=("protocol", "runtime"),
        history=(Interaction("obj:surface", 0.95, 1_100.0),),
    )
    recommendations = engine.recommend(
        user,
        (
            ContentProfile(
                "obj:surface",
                "Interactive protocol surfaces run inside signed object cards.",
                topics=("protocol", "runtime"),
                complexity_level=0.65,
                authenticity_score=0.9,
                temporal=TemporalInput("obj:surface", 1_000_000.0, quality_score=0.9),
            ),
            ContentProfile(
                "obj:recipe",
                "A short note about unrelated cooking technique.",
                topics=("cooking",),
                complexity_level=0.2,
                authenticity_score=0.4,
                temporal=TemporalInput("obj:recipe", 900_000.0, quality_score=0.4),
            ),
        ),
        peers=(peer,),
        reference_time=1_000_300.0,
    )

    assert recommendations[0].content_id == "obj:surface"
    assert recommendations[0].relevance > recommendations[1].relevance
    assert recommendations[0].collaborative > recommendations[1].collaborative


def test_feed_diversifier_prevents_creator_and_source_collapse() -> None:
    ranked = (
        ranked_candidate("obj:creator-a-1", 0.92, "Following"),
        ranked_candidate("obj:creator-a-2", 0.90, "Following"),
        ranked_candidate("obj:creator-a-3", 0.88, "Following"),
        ranked_candidate("obj:counter", 0.84, "Contradiction"),
        ranked_candidate("obj:wander", 0.80, "Exploration"),
    )
    trace = FeedDiversifier().diversify(
        ranked,
        (
            FeedObjectContext(ObjectId("obj:creator-a-1"), "creator:a", ("protocol",)),
            FeedObjectContext(ObjectId("obj:creator-a-2"), "creator:a", ("protocol",)),
            FeedObjectContext(ObjectId("obj:creator-a-3"), "creator:a", ("protocol",)),
            FeedObjectContext(ObjectId("obj:counter"), "creator:b", ("evidence",)),
            FeedObjectContext(ObjectId("obj:wander"), "creator:c", ("art",)),
        ),
        limit=4,
    )

    assert [item.ranked.candidate.object_id for item in trace.ranked] == [
        ObjectId("obj:creator-a-1"),
        ObjectId("obj:counter"),
        ObjectId("obj:wander"),
        ObjectId("obj:creator-a-2"),
    ]
    assert any(
        reason.signal == "source_floor" and reason.contribution > 0.0
        for reason in trace.ranked[1].reasons
    )
    assert trace.filtered == (ObjectId("obj:creator-a-3"),)


def test_feed_diversifier_penalizes_repeated_saturated_objects() -> None:
    repeated = ranked_candidate("obj:already-seen", 0.9, "SemanticNeighborhood")
    fresh = ranked_candidate("obj:fresh", 0.78, "SemanticNeighborhood")

    trace = FeedDiversifier().diversify(
        (repeated, fresh),
        (
            FeedObjectContext(
                ObjectId("obj:already-seen"), "creator:a", ("runtime",), seen_count=12
            ),
            FeedObjectContext(ObjectId("obj:fresh"), "creator:b", ("runtime",), seen_count=0),
        ),
    )

    assert trace.ranked[0].ranked.candidate.object_id == ObjectId("obj:fresh")
    assert any(reason.signal == "seen_saturation" for reason in trace.ranked[1].reasons)


def ranked_candidate(
    object_id: str,
    score: float,
    source: CandidateSource,
) -> RankedCandidate:
    return RankedCandidate(
        Candidate(ObjectId(object_id), source, signals=ObjectSignals()).normalized(),
        score,
        (),
    )
