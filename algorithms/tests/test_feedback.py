import math

import pytest

from babble_algorithms import (
    ContentProfile,
    RecommendationEngine,
    RecommendationFeedback,
    RecommendationWeights,
    UserProfile,
)


def test_feedback_changes_real_recommendation_order_without_mutating_shared_defaults() -> None:
    initial = RecommendationWeights(0.5, 0.0, 0.5, 0.0, 0.0)
    adapted = initial.with_feedback(
        (RecommendationFeedback(relevance=0.0, authenticity=1.0),), learning_rate=1.0
    )
    user = UserProfile("user", interests=("protocol",))
    content = (
        ContentProfile("relevant", "protocol", authenticity_score=0.2),
        ContentProfile("credible", "", authenticity_score=1.0),
    )
    before = RecommendationEngine(initial).recommend(user, content, reference_time=100.0)
    after = RecommendationEngine(adapted).recommend(user, content, reference_time=100.0)
    assert before[0].content_id == "relevant"
    assert after[0].content_id == "credible"
    assert initial == RecommendationWeights(0.5, 0.0, 0.5, 0.0, 0.0)


def test_feedback_averages_only_explicit_ratings_and_is_batch_order_independent() -> None:
    initial = RecommendationWeights()
    sparse = (RecommendationFeedback(relevance=1.0), RecommendationFeedback(engagement=0.0))
    result = initial.with_feedback(sparse)
    assert result == initial.with_feedback(tuple(reversed(sparse)))
    assert result == initial.with_feedback((RecommendationFeedback(relevance=1.0, engagement=0.0),))
    assert result.relevance > initial.relevance
    assert result.engagement < initial.engagement


def test_empty_neutral_and_disabled_feedback_preserve_normalized_weights() -> None:
    initial = RecommendationWeights().normalized()
    assert initial.with_feedback(()) == initial
    assert initial.with_feedback((RecommendationFeedback(),)) == initial
    assert initial.with_feedback((RecommendationFeedback(0.5, 0.5, 0.5),)) == initial
    assert initial.with_feedback((RecommendationFeedback(1.0, 0.0),), learning_rate=0.0) == initial


@pytest.mark.parametrize("value", [-0.01, 1.01, math.nan, math.inf, -math.inf])
def test_feedback_rejects_invalid_ratings(value: float) -> None:
    with pytest.raises(ValueError, match="feedback ratings"):
        _ = RecommendationFeedback(relevance=value)


@pytest.mark.parametrize("value", [-0.01, 1.01, math.nan, math.inf, -math.inf])
def test_feedback_rejects_invalid_learning_rates(value: float) -> None:
    with pytest.raises(ValueError, match="learning_rate"):
        _ = RecommendationWeights().with_feedback((), learning_rate=value)


def test_repeated_extreme_feedback_keeps_weights_bounded_and_normalized() -> None:
    weights = RecommendationWeights()
    for index in range(100):
        rating = float(index % 2)
        weights = weights.with_feedback((RecommendationFeedback(rating, 1.0 - rating, rating),))
        values = (
            weights.relevance, weights.engagement, weights.authenticity,
            weights.temporal, weights.collaborative,
        )
        assert all(math.isfinite(value) and 0.0 <= value <= 1.0 for value in values)
        assert sum(values) == pytest.approx(1.0)
