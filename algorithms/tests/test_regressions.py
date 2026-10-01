import math
from typing import cast

import pytest

from babble_algorithms.consensus import ConsensusAnalyzer, ConsensusSource, ConsensusState
from babble_algorithms.content import ContentAnalyzer
from babble_algorithms.discovery import CandidateEngine, DiscoveryRequest
from babble_algorithms.diversity import (
    DiversityPolicy,
    FeedDiversifier,
    FeedObjectContext,
    SourceFloor,
)
from babble_algorithms.judgment import JudgmentDefinition, LocalJudgmentProvider
from babble_algorithms.lens import BuiltInLens, LensStack, LensWeight
from babble_algorithms.recommendation import (
    ContentProfile,
    Interaction,
    RecommendationEngine,
    RecommendationFeedback,
    RecommendationWeights,
    UserProfile,
)
from babble_algorithms.text import tokens
from babble_algorithms.types import (
    Candidate,
    CandidateSource,
    CandidateSourceContribution,
    EvidenceSignals,
    ObjectId,
    ObjectSignals,
    RankedCandidate,
    ReputationSignals,
    clamp_score,
)


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


def test_candidate_source_weight_rejects_bool_by_sanitizing_to_zero() -> None:
    candidate = Candidate(
        ObjectId("object"),
        "Following",
        (CandidateSourceContribution("Following", cast(float, cast(object, True))),),
    ).normalized()
    assert candidate.sources[0].weight == 0.0


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
    huge = LensStack((LensWeight(BuiltInLens.RESEARCH, 1e308),) * 2)
    ordinary = LensStack((LensWeight(BuiltInLens.RESEARCH, 1.0),) * 2)
    assert huge.rank((candidate,)) == ordinary.rank((candidate,))


def test_lens_weights_reject_bool_domain_values() -> None:
    candidate = Candidate(ObjectId("object"), "Following")
    with pytest.raises(ValueError, match="lens weights"):
        _ = LensStack((LensWeight(BuiltInLens.RESEARCH, cast(float, cast(object, True))),)).rank(
            (candidate,)
        )


def test_content_analyzer_rejects_invalid_domain_inputs() -> None:
    analyzer = ContentAnalyzer()
    with pytest.raises(ValueError, match="content_id"):
        _ = analyzer.analyze("bad id", "text")
    with pytest.raises(ValueError, match="content text"):
        _ = analyzer.analyze("id", cast(str, cast(object, 123)))


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


def test_tokenizer_preserves_unicode_words() -> None:
    assert tokens("caf\u00e9 na\u00efve \u7814\u7a76 \u041d\u0430\u0443\u043a\u0430") == (
        "caf\u00e9",
        "na\u00efve",
        "\u7814\u7a76",
        "\u043d\u0430\u0443\u043a\u0430",
    )


def test_candidate_engine_rejects_invalid_supplied_ids_and_slots() -> None:
    with pytest.raises(ValueError, match="followed"):
        _ = CandidateEngine().candidates(DiscoveryRequest(followed=(ObjectId("bad id"),)))
    with pytest.raises(ValueError, match="exploration_slots"):
        _ = CandidateEngine().candidates(DiscoveryRequest(exploration_slots=-1))
    with pytest.raises(ValueError, match="exploration_slots"):
        _ = CandidateEngine().candidates(
            DiscoveryRequest(exploration_slots=cast(int, cast(object, True)))
        )


def test_feed_diversifier_rejects_ambiguous_domains() -> None:
    ranked = ranked_candidate("obj:one", 0.9, "Following")
    with pytest.raises(ValueError, match="ranked score"):
        _ = FeedDiversifier().diversify(
            (RankedCandidate(ranked.candidate, cast(float, cast(object, True)), ()),), ()
        )
    with pytest.raises(ValueError, match="limit"):
        _ = FeedDiversifier().diversify((ranked,), (), limit=cast(int, cast(object, True)))
    with pytest.raises(ValueError, match="creator_id"):
        _ = FeedDiversifier().diversify((ranked,), (FeedObjectContext(ObjectId("obj:one"), "  "),))
    with pytest.raises(ValueError, match="unknown candidate source"):
        _ = FeedDiversifier(
            DiversityPolicy(
                source_floors=(SourceFloor(cast(CandidateSource, cast(object, "Unknown")), 1),)
            )
        )


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
