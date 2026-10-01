import math
from typing import Literal, cast

import pytest

from babble_algorithms.consensus import ConsensusAnalyzer, ConsensusSource, ConsensusState
from babble_algorithms.content import (
    ContentAnalysis,
    ContentAnalyzer,
    EvidenceAnalysis,
    TextProperties,
)
from babble_algorithms.discovery import CandidateEngine, DiscoveryRequest
from babble_algorithms.diversity import (
    DiversifiedCandidate,
    DiversityPolicy,
    DiversityReason,
    DiversityTrace,
    FeedDiversifier,
    FeedObjectContext,
    SourceFloor,
)
from babble_algorithms.execution import (
    LEXICAL_LIMITATION,
    ContentOutput,
    HealthResult,
    JudgeResult,
    ModerationOutput,
    Provider,
    RelationshipOutput,
    ScoreOutput,
)
from babble_algorithms.judgment import JudgmentDefinition, LocalJudgmentProvider
from babble_algorithms.lens import BuiltInLens, LensStack, LensWeight
from babble_algorithms.moderation import ModerationAction, ModerationResult, ModerationScores
from babble_algorithms.ranking_types import RankingProvider
from babble_algorithms.recommendation import (
    ContentProfile,
    Interaction,
    RecommendationEngine,
    RecommendationFeedback,
    RecommendationScore,
    RecommendationWeights,
    UserProfile,
)
from babble_algorithms.temporal_types import TemporalProvider
from babble_algorithms.text import cosine, hashed_vector, sentences, tokens, top_terms
from babble_algorithms.types import (
    Candidate,
    CandidateSource,
    CandidateSourceContribution,
    EvidenceSignals,
    LensContribution,
    ObjectId,
    ObjectSignals,
    RankedCandidate,
    RankingTrace,
    ReputationSignals,
    clamp_score,
)
from babble_algorithms.wire import Definition


@pytest.mark.parametrize("value", [math.nan, math.inf, -math.inf])
def test_nonfinite_scores_never_become_positive_evidence(value: float) -> None:
    assert clamp_score(value) == 0.0


def test_bool_scores_never_become_positive_evidence() -> None:
    assert clamp_score(cast(float, cast(object, True))) == 0.0
    signals = ObjectSignals(
        relevance=cast(float, cast(object, True)),
        novelty=cast(float, cast(object, True)),
        evidence=EvidenceSignals(
            human_support=cast(float, cast(object, True)),
            judgment_contradiction=cast(float, cast(object, True)),
        ),
        reputation=ReputationSignals(
            social_constructiveness=cast(float, cast(object, True)),
            creative_contribution=cast(float, cast(object, True)),
        ),
    ).normalized()
    assert signals.relevance == 0.0
    assert signals.novelty == 0.0
    assert signals.evidence.support_score() == 0.0
    assert signals.evidence.contradiction_score() == 0.0
    assert signals.reputation.following_score() == 0.0


def test_candidate_source_weight_rejects_bool_at_the_boundary() -> None:
    with pytest.raises(ValueError, match="source contribution weight"):
        _ = CandidateSourceContribution("Following", cast(float, cast(object, True)))


def test_shared_algorithm_dtos_reject_invalid_structural_values() -> None:
    candidate = Candidate(ObjectId("object"), "Following").normalized()
    contribution = LensContribution("following", 1.0, 0.5, "valid reason")
    ranked = RankedCandidate(candidate, 0.5, (contribution,))
    assert RankingTrace((ranked,)).ranked == (ranked,)

    with pytest.raises(ValueError, match="candidate object_id"):
        _ = Candidate(ObjectId("bad id"), "Following")
    with pytest.raises(ValueError, match="unknown candidate source"):
        _ = CandidateSourceContribution(cast(CandidateSource, cast(object, "Unknown")), 1.0)
    with pytest.raises(ValueError, match="candidate sources must be a tuple"):
        _ = Candidate(
            ObjectId("object"),
            "Following",
            cast(
                tuple[CandidateSourceContribution, ...],
                cast(object, [CandidateSourceContribution("Following")]),
            ),
        )
    with pytest.raises(ValueError, match="duplicate candidate source"):
        _ = Candidate(
            ObjectId("object"),
            "Following",
            (
                CandidateSourceContribution("Following"),
                CandidateSourceContribution("Following", 0.5),
            ),
        )
    with pytest.raises(ValueError, match="include the primary source"):
        _ = Candidate(
            ObjectId("object"),
            "Following",
            (CandidateSourceContribution("Exploration"),),
        )
    with pytest.raises(ValueError, match="candidate signals"):
        _ = Candidate(ObjectId("object"), "Following", signals=cast(ObjectSignals, object()))
    with pytest.raises(ValueError, match="evidence must be EvidenceSignals"):
        _ = ObjectSignals(evidence=cast(EvidenceSignals, object()))
    with pytest.raises(ValueError, match="lens contribution weight"):
        _ = LensContribution("following", math.inf, 0.5, "reason")
    with pytest.raises(ValueError, match="lens contribution reason"):
        _ = LensContribution("following", 1.0, 0.5, "")
    with pytest.raises(ValueError, match="ranked score"):
        _ = RankedCandidate(candidate, cast(float, cast(object, True)), ())
    with pytest.raises(ValueError, match="ranked candidate contributions"):
        _ = RankedCandidate(candidate, 0.5, cast(tuple[LensContribution, ...], cast(object, [])))
    with pytest.raises(ValueError, match="duplicate ranking trace"):
        _ = RankingTrace((ranked, ranked))


def test_local_judgment_provider_rejects_invalid_domain_inputs() -> None:
    provider = LocalJudgmentProvider()
    with pytest.raises(ValueError, match="unsupported local Judgment definition"):
        _ = provider.judge(
            cast(JudgmentDefinition, cast(object, "babble.judgment.unknown.v1")), "text"
        )
    with pytest.raises(ValueError, match="text"):
        _ = provider.judge("babble.judgment.spam.v1", cast(str, cast(object, 123)))
    with pytest.raises(ValueError, match="context"):
        _ = provider.judge(
            "babble.judgment.relevance.v1", "text", context=cast(str, cast(object, []))
        )


def test_local_judgment_provider_validates_before_tokenizing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import babble_algorithms.judgment as judgment_module

    def explode(_text: str) -> tuple[str, ...]:
        raise AssertionError("tokenizer should not run for invalid context")

    monkeypatch.setattr(judgment_module, "_tokens", explode)
    with pytest.raises(ValueError, match="context"):
        _ = LocalJudgmentProvider().judge(
            "babble.judgment.relevance.v1",
            "text",
            context=cast(str, cast(object, False)),
        )


@pytest.mark.parametrize("value", [-1.0, math.nan, math.inf, -math.inf])
def test_recommendation_rejects_invalid_weights(value: float) -> None:
    with pytest.raises(ValueError, match="finite and non-negative"):
        _ = RecommendationEngine(RecommendationWeights(relevance=value))


def test_recommendation_weight_normalization_does_not_overflow() -> None:
    weights = RecommendationWeights(1e308, 1e308, 1e308, 1e308, 1e308).normalized()
    assert weights == RecommendationWeights(0.2, 0.2, 0.2, 0.2, 0.2)


def test_future_interactions_do_not_change_historical_recommendations() -> None:
    engine = RecommendationEngine()
    user = UserProfile("user", interests=("protocol",))
    peer = UserProfile("peer", interests=("protocol",))
    future = (Interaction("item", 1.0, 200.0),)
    content = (ContentProfile("item", "protocol", topics=("protocol",)),)
    baseline = engine.recommend(user, content, peers=(peer,), reference_time=100.0)
    result = engine.recommend(
        UserProfile("user", interests=("protocol",), history=future),
        content,
        peers=(UserProfile("peer", interests=("protocol",), history=future),),
        reference_time=100.0,
    )
    assert result == baseline


def test_recommendation_dtos_reject_invalid_direct_values() -> None:
    candidate = Candidate(ObjectId("item"), "Exploration").normalized()
    score = RecommendationScore("item", 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, candidate)
    assert score.candidate == candidate

    with pytest.raises(ValueError, match="user_id"):
        _ = UserProfile("")
    with pytest.raises(ValueError, match="interests"):
        _ = UserProfile("user", interests=cast(tuple[str, ...], cast(object, ["protocol"])))
    with pytest.raises(ValueError, match="history"):
        _ = UserProfile("user", history=cast(tuple[Interaction, ...], cast(object, (object(),))))
    with pytest.raises(ValueError, match="interaction content_id"):
        _ = Interaction("", 0.5, 0.0)
    with pytest.raises(ValueError, match="engagement_score"):
        _ = Interaction("item", math.nan, 0.0)
    with pytest.raises(ValueError, match="content_id"):
        _ = ContentProfile("", "text")
    with pytest.raises(ValueError, match="topics"):
        _ = ContentProfile("item", "text", topics=("",))
    with pytest.raises(ValueError, match="complexity_level"):
        _ = ContentProfile("item", "text", complexity_level=cast(float, cast(object, True)))
    with pytest.raises(ValueError, match="signals"):
        _ = ContentProfile("item", "text", signals=cast(ObjectSignals, object()))
    with pytest.raises(ValueError, match="finite and non-negative"):
        _ = RecommendationWeights(relevance=-0.1)
    with pytest.raises(ValueError, match="finite and non-negative"):
        _ = RecommendationWeights(collaborative=cast(float, cast(object, True)))
    with pytest.raises(ValueError, match="candidate object_id"):
        _ = RecommendationScore("other", 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, candidate)
    with pytest.raises(ValueError, match="candidate"):
        _ = RecommendationScore(
            "item", 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, cast(Candidate, object())
        )


def test_recommendation_engine_rejects_invalid_collection_boundaries() -> None:
    engine = RecommendationEngine()
    user = UserProfile("user")
    content = (ContentProfile("item", "text"),)
    with pytest.raises(ValueError, match="weights"):
        _ = RecommendationEngine(cast(RecommendationWeights, object()))
    with pytest.raises(ValueError, match="content"):
        _ = engine.recommend(
            user,
            cast(tuple[ContentProfile, ...], cast(object, [ContentProfile("item", "text")])),
            reference_time=0.0,
        )
    with pytest.raises(ValueError, match="content entries"):
        _ = engine.recommend(
            user,
            cast(tuple[ContentProfile, ...], cast(object, (object(),))),
            reference_time=0.0,
        )
    with pytest.raises(ValueError, match="peers"):
        _ = engine.recommend(
            user,
            content,
            peers=cast(tuple[UserProfile, ...], cast(object, [UserProfile("peer")])),
            reference_time=0.0,
        )
    with pytest.raises(ValueError, match="peer entries"):
        _ = engine.recommend(
            user,
            content,
            peers=cast(tuple[UserProfile, ...], cast(object, (object(),))),
            reference_time=0.0,
        )
    with pytest.raises(ValueError, match="limit"):
        _ = engine.recommend(user, content, reference_time=0.0, limit=cast(int, cast(object, True)))
    with pytest.raises(ValueError, match="feedback"):
        _ = RecommendationWeights().with_feedback(
            cast(tuple[RecommendationFeedback, ...], cast(object, [RecommendationFeedback()]))
        )
    with pytest.raises(ValueError, match="feedback entries"):
        _ = RecommendationWeights().with_feedback(
            cast(tuple[RecommendationFeedback, ...], cast(object, (object(),)))
        )


def test_recommendation_rejects_bool_numeric_inputs() -> None:
    with pytest.raises(ValueError, match="finite and non-negative"):
        _ = RecommendationEngine(RecommendationWeights(relevance=True))
    with pytest.raises(ValueError, match="feedback ratings"):
        _ = RecommendationFeedback(relevance=True)
    with pytest.raises(ValueError, match="learning_rate"):
        _ = RecommendationWeights().with_feedback((), learning_rate=True)
    with pytest.raises(ValueError, match="reference_time"):
        _ = RecommendationEngine().recommend(
            UserProfile("user"), (ContentProfile("item", "text"),), reference_time=True
        )


def test_recommendation_peer_features_are_built_once_per_peer(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    engine = RecommendationEngine()
    calls = 0
    original = engine._peer_features  # pyright: ignore[reportPrivateUsage]

    def counted(peer: UserProfile) -> object:
        nonlocal calls
        calls += 1
        return original(peer)

    monkeypatch.setattr(engine, "_peer_features", counted)
    user = UserProfile("user", interests=("protocol",))
    peers = tuple(
        UserProfile(
            f"peer-{index}",
            interests=("protocol",),
            history=(Interaction("item-0", 1.0, 0.0),),
        )
        for index in range(4)
    )
    content = tuple(
        ContentProfile(f"item-{index}", "protocol", topics=("protocol",)) for index in range(12)
    )
    first = engine.recommend(user, content, peers=peers, reference_time=1.0)
    second = RecommendationEngine().recommend(user, content, peers=peers, reference_time=1.0)
    assert first == second
    assert calls == len(peers)


def test_candidate_sources_do_not_replace_observed_signals_with_priors() -> None:
    object_id = ObjectId("observed")
    observed = ObjectSignals(relevance=0.12, novelty=0.03, contradiction=0.27)
    candidates = CandidateEngine().candidates(
        DiscoveryRequest(
            followed=(object_id,),
            contradicting_evidence=(object_id,),
            object_signals={object_id: observed},
        )
    )
    assert candidates[0].signals == observed
    assert {source.source for source in candidates[0].sources} == {"Following", "Contradiction"}


def test_exploration_does_not_invent_novelty_or_weirdness() -> None:
    observed = ObjectSignals(novelty=0.0, weirdness=0.0)
    candidates = CandidateEngine().candidates(
        DiscoveryRequest(object_signals={ObjectId("known"): observed})
    )
    assert candidates[0].signals == observed


def test_single_source_cannot_establish_agreement_with_itself() -> None:
    source = ConsensusSource(
        "only",
        "official_docs",
        "The system has evidence.",
        100.0,
        quality_score=1.0,
        evidence_score=1.0,
        vote=1.0,
    )
    result = ConsensusAnalyzer().evaluate("claim", (source,), reference_time=100.0)
    assert result.term_agreement == result.fact_agreement == 0.0
    assert result.state == ConsensusState.INSUFFICIENT


def test_empty_sources_revoke_previously_established_consensus() -> None:
    result = ConsensusAnalyzer().evaluate("claim", (), reference_time=100.0, previous_score=0.9)
    assert result.state == ConsensusState.REVOKED


def test_duplicate_source_ids_cannot_inflate_validation_count() -> None:
    source = ConsensusSource("same", "official_docs", "The system has evidence.", 100.0)
    with pytest.raises(ValueError, match="unique"):
        _ = ConsensusAnalyzer().evaluate("claim", (source, source), reference_time=100.0)


def test_lens_normalizes_individual_signals_before_combining() -> None:
    raw = Candidate(ObjectId("bad"), "Following", signals=ObjectSignals(recency=1000.0))
    result = LensStack.following().rank((raw,))
    expected = LensStack.following().rank((raw.normalized(),))
    assert result == expected


def test_lens_weight_normalization_does_not_overflow() -> None:
    candidate = Candidate(ObjectId("object"), "Following")
    huge = LensStack(
        (
            LensWeight(BuiltInLens.RESEARCH, 1e308),
            LensWeight(BuiltInLens.WEIRD, 1e308),
        )
    )
    ordinary = LensStack(
        (
            LensWeight(BuiltInLens.RESEARCH, 1.0),
            LensWeight(BuiltInLens.WEIRD, 1.0),
        )
    )
    assert huge.rank((candidate,)) == ordinary.rank((candidate,))


def test_lens_weights_reject_bool_domain_values() -> None:
    candidate = Candidate(ObjectId("object"), "Following")
    with pytest.raises(ValueError, match="lens weights"):
        _ = LensStack((LensWeight(BuiltInLens.RESEARCH, cast(float, cast(object, True))),)).rank(
            (candidate,)
        )


def test_lens_direct_boundaries_reject_ambiguous_values() -> None:
    candidate = Candidate(ObjectId("object"), "Following")
    with pytest.raises(ValueError, match="built-in lens"):
        _ = LensWeight(cast(BuiltInLens, cast(object, "research")), 1.0)
    with pytest.raises(ValueError, match="lens weights"):
        _ = LensStack(
            cast(tuple[LensWeight, ...], cast(object, [LensWeight(BuiltInLens.RESEARCH, 1.0)]))
        )
    with pytest.raises(ValueError, match="duplicate lens"):
        _ = LensStack(
            (
                LensWeight(BuiltInLens.RESEARCH, 0.7),
                LensWeight(BuiltInLens.RESEARCH, 0.3),
            )
        )
    with pytest.raises(ValueError, match="candidates"):
        _ = LensStack.following().rank(cast(tuple[Candidate, ...], cast(object, [candidate])))
    with pytest.raises(ValueError, match="Candidate"):
        _ = LensStack.following().rank((cast(Candidate, object()),))
    with pytest.raises(ValueError, match="duplicate candidate"):
        _ = LensStack.following().rank((candidate, candidate))


def test_content_analyzer_rejects_invalid_domain_inputs() -> None:
    analyzer = ContentAnalyzer()
    with pytest.raises(ValueError, match="content_id"):
        _ = analyzer.analyze("bad id", "text")
    with pytest.raises(ValueError, match="content text"):
        _ = analyzer.analyze("id", cast(str, cast(object, 123)))


def test_content_dtos_reject_invalid_direct_values() -> None:
    properties = TextProperties(1, 2, 2, 2.0, 1.0)
    evidence = EvidenceAnalysis(1, 0.2, ("dataset",), ("Dataset confirms it",))
    valid = ContentAnalysis(
        "id", properties, {"science": 1.0}, evidence, 0.5, 0.0, "Summary.", ("dataset",)
    )
    assert valid.topics == {"science": 1.0}

    with pytest.raises(ValueError):
        _ = TextProperties(cast(int, cast(object, True)), 0, 0, 0.0, 0.0)
    with pytest.raises(ValueError):
        _ = TextProperties(0, 1, 1, 0.0, 1.0)
    with pytest.raises(ValueError):
        _ = TextProperties(1, 1, 2, 1.0, 1.0)
    with pytest.raises(ValueError):
        _ = TextProperties(1, 1, 1, math.nan, 1.0)
    with pytest.raises(ValueError):
        _ = TextProperties(1, 1, 1, 1.0, 1.01)

    with pytest.raises(ValueError):
        _ = EvidenceAnalysis(2, 0.2, ("dataset",), ())
    with pytest.raises(ValueError):
        _ = EvidenceAnalysis(1, math.nan, ("dataset",), ())
    with pytest.raises(ValueError):
        _ = EvidenceAnalysis(1, 0.2, cast(tuple[str, ...], cast(object, ["dataset"])), ())
    with pytest.raises(ValueError):
        _ = EvidenceAnalysis(1, 0.2, ("",), ())
    with pytest.raises(ValueError):
        _ = EvidenceAnalysis(0, 0.0, (), (cast(str, cast(object, 1)),))

    with pytest.raises(ValueError):
        _ = ContentAnalysis("bad id", properties, {}, evidence, 0.5, 0.0, "Summary.", ())
    with pytest.raises(ValueError):
        _ = ContentAnalysis(
            "id", cast(TextProperties, object()), {}, evidence, 0.5, 0.0, "Summary.", ()
        )
    with pytest.raises(ValueError):
        _ = ContentAnalysis(
            "id", properties, {"science": math.inf}, evidence, 0.5, 0.0, "Summary.", ()
        )
    with pytest.raises(ValueError):
        _ = ContentAnalysis("id", properties, {"": 0.0}, evidence, 0.5, 0.0, "Summary.", ())
    with pytest.raises(ValueError):
        _ = ContentAnalysis(
            "id", properties, {}, cast(EvidenceAnalysis, object()), 0.5, 0.0, "Summary.", ()
        )
    with pytest.raises(ValueError):
        _ = ContentAnalysis("id", properties, {}, evidence, -0.01, 0.0, "Summary.", ())
    with pytest.raises(ValueError):
        _ = ContentAnalysis("id", properties, {}, evidence, 0.5, -1.01, "Summary.", ())
    with pytest.raises(ValueError):
        _ = ContentAnalysis(
            "id", properties, {}, evidence, 0.5, 0.0, cast(str, cast(object, 1)), ()
        )
    with pytest.raises(ValueError):
        _ = ContentAnalysis("id", properties, {}, evidence, 0.5, 0.0, "Summary.", ("",))


def test_moderation_dtos_reject_invalid_direct_values() -> None:
    scores = ModerationScores(0.1, 0.9, -0.2, 0.0, 0.3)
    result = ModerationResult("id", "allow", (), scores, ("no moderation thresholds exceeded",))
    assert result.scores == scores

    with pytest.raises(ValueError):
        _ = ModerationScores(cast(float, cast(object, True)), 0.9, 0.0, 0.0, 0.0)
    with pytest.raises(ValueError):
        _ = ModerationScores(0.1, math.nan, 0.0, 0.0, 0.0)
    with pytest.raises(ValueError):
        _ = ModerationScores(0.1, 0.9, -1.01, 0.0, 0.0)
    with pytest.raises(ValueError):
        _ = ModerationScores(0.1, 0.9, 0.0, 1.01, 0.0)
    with pytest.raises(ValueError):
        _ = ModerationScores(0.1, 0.9, 0.0, 0.0, math.inf)

    with pytest.raises(ValueError):
        _ = ModerationResult("bad id", "allow", (), scores, ("reason",))
    with pytest.raises(ValueError):
        _ = ModerationResult(
            "id",
            cast(ModerationAction, cast(object, "unknown")),
            (),
            scores,
            ("reason",),
        )
    with pytest.raises(ValueError):
        _ = ModerationResult(
            "id",
            "allow",
            cast(tuple[str, ...], cast(object, ["spam"])),
            scores,
            ("reason",),
        )
    with pytest.raises(ValueError):
        _ = ModerationResult("id", "allow", ("",), scores, ("reason",))
    with pytest.raises(ValueError):
        _ = ModerationResult("id", "allow", (), cast(ModerationScores, object()), ("reason",))
    with pytest.raises(ValueError):
        _ = ModerationResult(
            "id",
            "allow",
            (),
            scores,
            cast(tuple[str, ...], cast(object, ["reason"])),
        )
    with pytest.raises(ValueError):
        _ = ModerationResult("id", "allow", (), scores, ("",))


def test_content_evidence_references_use_precomputed_sentence_folds() -> None:
    analysis = ContentAnalyzer().analyze(
        "id",
        "Intro. According to the DATASET, replication works. Methodology confirms it.",
    )
    assert analysis.evidence.markers_found == (
        "according to",
        "dataset",
        "methodology",
        "replication",
    )
    assert analysis.evidence.references == (
        "According to the DATASET, replication works",
        "Methodology confirms it",
    )


def test_text_helpers_reject_invalid_domain_inputs() -> None:
    with pytest.raises(ValueError, match="text"):
        _ = tokens(cast(str, cast(object, True)))
    with pytest.raises(ValueError, match="text"):
        _ = sentences(cast(str, cast(object, 123)))
    with pytest.raises(ValueError, match="remove_stop_words"):
        _ = tokens("hello", remove_stop_words=cast(bool, cast(object, 1)))
    with pytest.raises(ValueError, match="limit"):
        _ = top_terms("hello", limit=cast(int, cast(object, True)))


def test_vector_helpers_reject_invalid_domains_and_nonfinite_similarity() -> None:
    with pytest.raises(ValueError, match="vector dimensions"):
        _ = hashed_vector(("term",), dimensions=cast(int, cast(object, True)))
    with pytest.raises(ValueError, match="vector terms"):
        _ = hashed_vector((cast(str, cast(object, 1)),))
    assert cosine((1.0, float("nan")), (1.0, 1.0)) == 0.0
    assert cosine((cast(float, cast(object, True)),), (1.0,)) == 0.0


def test_tokenizer_preserves_unicode_words() -> None:
    assert tokens("caf\u00e9 na\u00efve \u7814\u7a76 \u041d\u0430\u0443\u043a\u0430") == (
        "caf\u00e9",
        "na\u00efve",
        "\u7814\u7a76",
        "\u043d\u0430\u0443\u043a\u0430",
    )


def test_candidate_engine_rejects_invalid_supplied_ids_and_slots() -> None:
    with pytest.raises(ValueError, match="DiscoveryRequest"):
        _ = CandidateEngine().candidates(cast(DiscoveryRequest, object()))
    with pytest.raises(ValueError, match="followed"):
        _ = CandidateEngine().candidates(DiscoveryRequest(followed=(ObjectId("bad id"),)))
    with pytest.raises(ValueError, match="followed"):
        _ = CandidateEngine().candidates(
            DiscoveryRequest(followed=cast(tuple[ObjectId, ...], cast(object, [ObjectId("id")]))),
        )
    with pytest.raises(ValueError, match="object_signals"):
        _ = CandidateEngine().candidates(
            DiscoveryRequest(object_signals=cast(dict[ObjectId, ObjectSignals], cast(object, ())))
        )
    with pytest.raises(ValueError, match="object_signals values"):
        _ = CandidateEngine().candidates(
            DiscoveryRequest(object_signals={ObjectId("id"): cast(ObjectSignals, object())})
        )
    with pytest.raises(ValueError, match="object_id"):
        _ = CandidateEngine().candidates(
            DiscoveryRequest(object_signals={ObjectId("bad id"): ObjectSignals()})
        )
    with pytest.raises(ValueError, match="exploration_slots"):
        _ = CandidateEngine().candidates(DiscoveryRequest(exploration_slots=-1))
    with pytest.raises(ValueError, match="exploration_slots"):
        _ = CandidateEngine().candidates(
            DiscoveryRequest(exploration_slots=cast(int, cast(object, True)))
        )


def test_feed_diversifier_rejects_ambiguous_domains() -> None:
    ranked = ranked_candidate("obj:one", 0.9, "Following")
    context = FeedObjectContext(ObjectId("obj:one"), "creator", ("Topic", "topic"), 0)
    assert context.topics == ("topic",)

    with pytest.raises(ValueError, match="ranked score"):
        _ = FeedDiversifier().diversify(
            (RankedCandidate(ranked.candidate, cast(float, cast(object, True)), ()),), ()
        )
    with pytest.raises(ValueError, match="ranked candidates"):
        _ = FeedDiversifier().diversify(
            cast(tuple[RankedCandidate, ...], cast(object, [ranked])), ()
        )
    with pytest.raises(ValueError, match="duplicate ranked candidate"):
        _ = FeedDiversifier().diversify((ranked, ranked), ())
    with pytest.raises(ValueError, match="contexts"):
        _ = FeedDiversifier().diversify(
            (ranked,), cast(tuple[FeedObjectContext, ...], cast(object, [context]))
        )
    with pytest.raises(ValueError, match="duplicate context"):
        _ = FeedDiversifier().diversify((ranked,), (context, context))
    with pytest.raises(ValueError, match="limit"):
        _ = FeedDiversifier().diversify((ranked,), (), limit=cast(int, cast(object, True)))
    with pytest.raises(ValueError, match="creator_id"):
        _ = FeedObjectContext(ObjectId("obj:one"), "  ")
    with pytest.raises(ValueError, match="topics"):
        _ = FeedObjectContext(
            ObjectId("obj:one"), "creator", cast(tuple[str, ...], cast(object, ["topic"]))
        )
    with pytest.raises(ValueError, match="seen_count"):
        _ = FeedObjectContext(ObjectId("obj:one"), "creator", (), -1)
    with pytest.raises(ValueError, match="unknown candidate source"):
        _ = SourceFloor(cast(CandidateSource, cast(object, "Unknown")), 1)
    with pytest.raises(ValueError, match="source floor minimum"):
        _ = SourceFloor("Following", -1)
    with pytest.raises(ValueError, match="source_floors"):
        _ = DiversityPolicy(
            source_floors=cast(tuple[SourceFloor, ...], cast(object, [SourceFloor("Following", 1)]))
        )
    with pytest.raises(ValueError, match="diversity reason signal"):
        _ = DiversityReason("", 0.0)
    with pytest.raises(ValueError, match="diversity reason contribution"):
        _ = DiversityReason("source_floor", math.nan)
    with pytest.raises(ValueError, match="ranked"):
        _ = DiversifiedCandidate(cast(RankedCandidate, object()), 0.5, ())
    with pytest.raises(ValueError, match="diversity reasons"):
        _ = DiversifiedCandidate(ranked, 0.5, cast(tuple[DiversityReason, ...], cast(object, [])))
    with pytest.raises(ValueError, match="diversified candidates"):
        _ = DiversityTrace(cast(tuple[DiversifiedCandidate, ...], cast(object, [])), ())
    with pytest.raises(ValueError, match="filtered"):
        _ = DiversityTrace((), cast(tuple[ObjectId, ...], cast(object, [ObjectId("obj:two")])))


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


def test_execution_score_outputs_reject_invalid_direct_values() -> None:
    valid = ScoreOutput(
        "probability",
        0.5,
        0.0,
        "legacy_heuristic",
        "not_spam",
        ("no spam markers",),
        (LEXICAL_LIMITATION,),
    )
    assert valid.score == 0.5

    with pytest.raises(ValueError, match="score kind"):
        _ = ScoreOutput(
            cast(Literal["probability", "bounded_score"], cast(object, "unknown")),
            0.5,
            0.0,
            "legacy_heuristic",
            "label",
            ("reason",),
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="score"):
        _ = ScoreOutput(
            "probability",
            math.nan,
            0.0,
            "legacy_heuristic",
            "label",
            ("reason",),
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="confidence_status"):
        _ = ScoreOutput(
            "probability",
            0.5,
            0.0,
            cast(Literal["legacy_heuristic"], cast(object, "calibrated")),
            "label",
            ("reason",),
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="label"):
        _ = ScoreOutput(
            "probability", 0.5, 0.0, "legacy_heuristic", " ", ("reason",), (LEXICAL_LIMITATION,)
        )
    with pytest.raises(ValueError, match="reasons"):
        _ = ScoreOutput(
            "probability",
            0.5,
            0.0,
            "legacy_heuristic",
            "label",
            cast(tuple[str, ...], cast(object, ["reason"])),
            (LEXICAL_LIMITATION,),
        )


def test_execution_relationship_outputs_reject_invalid_direct_values() -> None:
    valid = RelationshipOutput(
        "relationship",
        "supports",
        0.8,
        0.2,
        "legacy_heuristic",
        "supports",
        ("support marker found",),
        False,
        "text",
        (LEXICAL_LIMITATION,),
    )
    assert valid.relation == "supports"

    with pytest.raises(ValueError, match="relationship relation"):
        _ = RelationshipOutput(
            "relationship",
            cast(Literal["supports", "contradicts", "related"], cast(object, "opposes")),
            0.8,
            0.2,
            "legacy_heuristic",
            "supports",
            ("reason",),
            False,
            "text",
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="target_context_evaluated"):
        _ = RelationshipOutput(
            "relationship",
            "supports",
            0.8,
            0.2,
            "legacy_heuristic",
            "supports",
            ("reason",),
            cast(bool, cast(object, 1)),
            "text",
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="marker_scope"):
        _ = RelationshipOutput(
            "relationship",
            "supports",
            0.8,
            0.2,
            "legacy_heuristic",
            "supports",
            ("reason",),
            False,
            cast(Literal["source_text", "text"], cast(object, "document")),
            (LEXICAL_LIMITATION,),
        )


def test_execution_content_outputs_reject_invalid_direct_values() -> None:
    properties = TextProperties(1, 2, 2, 2.0, 1.0)
    valid = ContentOutput(
        "content_analysis",
        ("science",),
        ("dataset",),
        ("dataset",),
        "Summary.",
        0.5,
        0.0,
        "uncalibrated",
        properties,
        {"science": 1.0},
        0.4,
        0.2,
        (LEXICAL_LIMITATION,),
    )
    assert valid.topic_scores == {"science": 1.0}

    with pytest.raises(ValueError, match="content output kind"):
        _ = ContentOutput(
            cast(Literal["content_analysis"], cast(object, "content")),
            (),
            (),
            (),
            "Summary.",
            0.5,
            0.0,
            "uncalibrated",
            properties,
            {},
            0.4,
            0.2,
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="properties"):
        _ = ContentOutput(
            "content_analysis",
            (),
            (),
            (),
            "Summary.",
            0.5,
            0.0,
            "uncalibrated",
            cast(TextProperties, object()),
            {},
            0.4,
            0.2,
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="topic_scores"):
        _ = ContentOutput(
            "content_analysis",
            (),
            (),
            (),
            "Summary.",
            0.5,
            0.0,
            "uncalibrated",
            properties,
            cast(dict[str, float], cast(object, (("science", 1.0),))),
            0.4,
            0.2,
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="topic"):
        _ = ContentOutput(
            "content_analysis",
            (),
            (),
            (),
            "Summary.",
            0.5,
            0.0,
            "uncalibrated",
            properties,
            {"": 1.0},
            0.4,
            0.2,
            (LEXICAL_LIMITATION,),
        )


def test_execution_moderation_outputs_reject_invalid_direct_values() -> None:
    valid = ModerationOutput(
        "moderation",
        "allow",
        "allow",
        (),
        0.1,
        0.9,
        1.0,
        0.0,
        0.0,
        0.5,
        0.0,
        "uncalibrated",
        ("no moderation thresholds exceeded",),
        (LEXICAL_LIMITATION,),
    )
    assert valid.action == "allow"

    with pytest.raises(ValueError, match="moderation action"):
        _ = ModerationOutput(
            "moderation",
            cast(Literal["allow", "limit", "flag", "remove"], cast(object, "warn")),
            "allow",
            (),
            0.1,
            0.9,
            1.0,
            0.0,
            0.0,
            0.5,
            0.0,
            "uncalibrated",
            ("reason",),
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="advisory action"):
        _ = ModerationOutput(
            "moderation",
            "allow",
            cast(ModerationAction, cast(object, "escalate")),
            (),
            0.1,
            0.9,
            1.0,
            0.0,
            0.0,
            0.5,
            0.0,
            "uncalibrated",
            ("reason",),
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="misinformation"):
        _ = ModerationOutput(
            "moderation",
            "allow",
            "allow",
            (),
            0.1,
            0.9,
            1.0,
            0.0,
            math.inf,
            0.5,
            0.0,
            "uncalibrated",
            ("reason",),
            (LEXICAL_LIMITATION,),
        )
    with pytest.raises(ValueError, match="reasons"):
        _ = ModerationOutput(
            "moderation",
            "allow",
            "allow",
            (),
            0.1,
            0.9,
            1.0,
            0.0,
            0.0,
            0.5,
            0.0,
            "uncalibrated",
            ("",),
            (LEXICAL_LIMITATION,),
        )


def test_execution_result_wrappers_reject_invalid_direct_values() -> None:
    score = ScoreOutput(
        "bounded_score",
        0.7,
        0.0,
        "legacy_heuristic",
        "relevant",
        ("query tokens overlap",),
        (LEXICAL_LIMITATION,),
    )
    assert JudgeResult(Provider(), score, 0.0).output == score

    with pytest.raises(ValueError, match="provider"):
        _ = Provider(cast(Literal["babble-python"], cast(object, "other")), "lexical-v1", "1")
    with pytest.raises(ValueError, match="provider"):
        _ = JudgeResult(cast(Provider, object()), score, 0.0)
    with pytest.raises(ValueError, match="output"):
        _ = JudgeResult(Provider(), cast(ScoreOutput, object()), 0.0)
    with pytest.raises(ValueError, match="confidence"):
        _ = JudgeResult(Provider(), score, math.nan)
    with pytest.raises(ValueError, match="supported_definitions"):
        _ = HealthResult(
            Provider(),
            cast(tuple[Definition, ...], ("unknown",)),
            cast(RankingProvider, object()),
            cast(TemporalProvider, object()),
        )
