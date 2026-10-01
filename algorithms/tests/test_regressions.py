import math

import pytest

from babble_algorithms.consensus import ConsensusAnalyzer, ConsensusSource, ConsensusState
from babble_algorithms.discovery import CandidateEngine, DiscoveryRequest
from babble_algorithms.lens import BuiltInLens, LensStack, LensWeight
from babble_algorithms.recommendation import (
    ContentProfile,
    Interaction,
    RecommendationEngine,
    RecommendationWeights,
    UserProfile,
)
from babble_algorithms.text import tokens
from babble_algorithms.types import Candidate, ObjectId, ObjectSignals, clamp_score


@pytest.mark.parametrize("value", [math.nan, math.inf, -math.inf])
def test_nonfinite_scores_never_become_positive_evidence(value: float) -> None:
    assert clamp_score(value) == 0.0


@pytest.mark.parametrize("value", [-1.0, math.nan, math.inf, -math.inf])
def test_recommendation_rejects_invalid_weights(value: float) -> None:
    with pytest.raises(ValueError, match="finite and non-negative"):
        RecommendationEngine(RecommendationWeights(relevance=value))


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
        "only", "official_docs", "The system has evidence.", 100.0,
        quality_score=1.0, evidence_score=1.0, vote=1.0,
    )
    result = ConsensusAnalyzer().evaluate("claim", (source,), reference_time=100.0)
    assert result.term_agreement == result.fact_agreement == 0.0
    assert result.state == ConsensusState.INSUFFICIENT


def test_empty_sources_revoke_previously_established_consensus() -> None:
    result = ConsensusAnalyzer().evaluate(
        "claim", (), reference_time=100.0, previous_score=0.9
    )
    assert result.state == ConsensusState.REVOKED


def test_duplicate_source_ids_cannot_inflate_validation_count() -> None:
    source = ConsensusSource("same", "official_docs", "The system has evidence.", 100.0)
    with pytest.raises(ValueError, match="unique"):
        ConsensusAnalyzer().evaluate("claim", (source, source), reference_time=100.0)


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


def test_tokenizer_preserves_unicode_words() -> None:
    assert tokens("caf\u00e9 na\u00efve \u7814\u7a76 \u041d\u0430\u0443\u043a\u0430") == (
        "caf\u00e9", "na\u00efve", "\u7814\u7a76", "\u043d\u0430\u0443\u043a\u0430"
    )
