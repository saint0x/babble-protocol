from __future__ import annotations

import math
from dataclasses import dataclass, field, replace

from babble_algorithms.temporal import TemporalInput, TemporalScorer
from babble_algorithms.text import cosine, hashed_vector, tokens
from babble_algorithms.types import Candidate, ObjectId, ObjectSignals, clamp_score


def _number(value: object, name: str, *, unit: bool = False, nonnegative: bool = False) -> float:
    def invalid() -> ValueError:
        if nonnegative:
            return ValueError(f"{name} must be finite and non-negative")
        return ValueError(f"{name} must be a finite real number")

    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise invalid()
    try:
        number = float(value)
    except OverflowError as error:
        raise invalid() from error
    if not math.isfinite(number):
        raise invalid()
    if unit and not 0.0 <= number <= 1.0:
        raise ValueError(f"{name} must be between zero and one")
    if nonnegative and number < 0.0:
        raise invalid()
    return number


def _vector(values: tuple[str, ...]) -> tuple[float, ...]:
    return hashed_vector(tokens(" ".join(values), remove_stop_words=True))


@dataclass(frozen=True, slots=True)
class UserProfile:
    user_id: str
    interests: tuple[str, ...] = ()
    expertise: tuple[str, ...] = ()
    history: tuple[Interaction, ...] = ()


@dataclass(frozen=True, slots=True)
class Interaction:
    content_id: str
    engagement_score: float
    timestamp: float


@dataclass(frozen=True, slots=True)
class ContentProfile:
    content_id: str
    text: str
    topics: tuple[str, ...] = ()
    complexity_level: float = 0.5
    authenticity_score: float = 0.5
    temporal: TemporalInput | None = None
    signals: ObjectSignals = field(default_factory=ObjectSignals)


@dataclass(frozen=True, slots=True)
class RecommendationFeedback:
    """Explicit ratings in [0, 1]; omitted dimensions do not imply a negative rating."""

    relevance: float | None = None
    engagement: float | None = None
    authenticity: float | None = None

    def __post_init__(self) -> None:
        for field_name, value in (
            ("relevance", self.relevance),
            ("engagement", self.engagement),
            ("authenticity", self.authenticity),
        ):
            if value is not None:
                object.__setattr__(self, field_name, _number(value, "feedback ratings", unit=True))


@dataclass(frozen=True, slots=True)
class RecommendationWeights:
    relevance: float = 0.3
    engagement: float = 0.2
    authenticity: float = 0.2
    temporal: float = 0.15
    collaborative: float = 0.15

    def normalized(self) -> RecommendationWeights:
        values = (
            _number(self.relevance, "recommendation weights", nonnegative=True),
            _number(self.engagement, "recommendation weights", nonnegative=True),
            _number(self.authenticity, "recommendation weights", nonnegative=True),
            _number(self.temporal, "recommendation weights", nonnegative=True),
            _number(self.collaborative, "recommendation weights", nonnegative=True),
        )
        scale = max(values)
        if scale == 0.0:
            return RecommendationWeights()
        total = math.fsum(value / scale for value in values)
        return RecommendationWeights(
            relevance=(values[0] / scale) / total,
            engagement=(values[1] / scale) / total,
            authenticity=(values[2] / scale) / total,
            temporal=(values[3] / scale) / total,
            collaborative=(values[4] / scale) / total,
        )

    def with_feedback(
        self,
        feedback: tuple[RecommendationFeedback, ...],
        *,
        learning_rate: float = 0.1,
    ) -> RecommendationWeights:
        """Apply one new batch to caller-owned weights, without retaining private history.

        This is bounded explicit-feedback adaptation, not a learned relevance model.
        Callers own per-user persistence and must avoid replaying an already applied batch.
        """
        rate = _number(learning_rate, "learning_rate", unit=True)
        base = self.normalized()

        def adjusted(weight: float, ratings: tuple[float | None, ...]) -> float:
            present = tuple(rating for rating in ratings if rating is not None)
            if not present:
                return weight
            delta = rate * math.fsum(rating - 0.5 for rating in present) / len(present)
            return clamp_score(weight + delta)

        return RecommendationWeights(
            relevance=adjusted(base.relevance, tuple(item.relevance for item in feedback)),
            engagement=adjusted(base.engagement, tuple(item.engagement for item in feedback)),
            authenticity=adjusted(base.authenticity, tuple(item.authenticity for item in feedback)),
            temporal=base.temporal,
            collaborative=base.collaborative,
        ).normalized()


@dataclass(frozen=True, slots=True)
class RecommendationScore:
    content_id: str
    final_score: float
    confidence: float
    relevance: float
    engagement: float
    authenticity: float
    temporal: float
    collaborative: float
    candidate: Candidate


@dataclass(frozen=True, slots=True)
class _PeerFeatures:
    user_id: str
    vector: tuple[float, ...]
    history_by_content: dict[str, tuple[float, ...]]


@dataclass(frozen=True, slots=True)
class _RecommendationContext:
    user: UserProfile
    peers: tuple[UserProfile, ...]
    user_terms: tuple[str, ...]
    user_interest_terms: set[str]
    user_vector: tuple[float, ...]
    peer_features: tuple[_PeerFeatures, ...]


class RecommendationEngine:
    def __init__(self, weights: RecommendationWeights | None = None) -> None:
        self.weights: RecommendationWeights = (weights or RecommendationWeights()).normalized()
        self.temporal: TemporalScorer = TemporalScorer()

    def recommend(
        self,
        user: UserProfile,
        content: tuple[ContentProfile, ...],
        *,
        peers: tuple[UserProfile, ...] = (),
        reference_time: float,
        limit: int | None = None,
    ) -> tuple[RecommendationScore, ...]:
        reference_time = _number(reference_time, "reference_time")
        context = self._context(user, peers, reference_time)
        ranked = tuple(
            sorted(
                (
                    self._score(item, context=context, reference_time=reference_time)
                    for item in content
                ),
                key=lambda score: score.final_score,
                reverse=True,
            )
        )
        if limit is None:
            return ranked
        return ranked[: max(0, limit)]

    def _context(
        self, user: UserProfile, peers: tuple[UserProfile, ...], reference_time: float
    ) -> _RecommendationContext:
        scoped_user = _profile_at(user, reference_time)
        scoped_peers = tuple(_profile_at(peer, reference_time) for peer in peers)
        user_terms = tokens(
            " ".join(scoped_user.interests + scoped_user.expertise), remove_stop_words=True
        )
        user_interest_terms = set(tokens(" ".join(scoped_user.interests), remove_stop_words=True))
        user_vector = hashed_vector(user_terms)
        peer_features = tuple(
            self._peer_features(peer)
            for peer in scoped_peers
            if peer.user_id != scoped_user.user_id
        )
        return _RecommendationContext(
            scoped_user, scoped_peers, user_terms, user_interest_terms, user_vector, peer_features
        )

    def _peer_features(self, peer: UserProfile) -> _PeerFeatures:
        history: dict[str, list[float]] = {}
        for interaction in peer.history:
            history.setdefault(interaction.content_id, []).append(
                clamp_score(interaction.engagement_score)
            )
        return _PeerFeatures(
            peer.user_id,
            _vector(peer.interests + peer.expertise),
            {content_id: tuple(scores) for content_id, scores in history.items()},
        )

    def _score(
        self,
        content: ContentProfile,
        *,
        context: _RecommendationContext,
        reference_time: float,
    ) -> RecommendationScore:
        relevance = self._relevance(context, content)
        engagement = self._engagement_prediction(context, content)
        authenticity = clamp_score(content.authenticity_score)
        temporal = self._temporal_score(content, reference_time)
        collaborative = self._collaborative(content.content_id, context)
        final = clamp_score(
            self.weights.relevance * relevance
            + self.weights.engagement * engagement
            + self.weights.authenticity * authenticity
            + self.weights.temporal * temporal
            + self.weights.collaborative * collaborative
        )
        confidence = self._confidence(context.user, content, context.peers)
        signals = ObjectSignals(
            relevance=relevance,
            novelty=max(0.0, 1.0 - engagement),
            evidence_quality=authenticity,
            recency=temporal,
            reputation=content.signals.reputation,
            evidence=content.signals.evidence,
            contradiction=content.signals.contradiction,
            social_distance=content.signals.social_distance,
            emerging=content.signals.emerging,
            weirdness=content.signals.weirdness,
        ).normalized()
        return RecommendationScore(
            content_id=content.content_id,
            final_score=final,
            confidence=confidence,
            relevance=relevance,
            engagement=engagement,
            authenticity=authenticity,
            temporal=temporal,
            collaborative=collaborative,
            candidate=Candidate(
                ObjectId(content.content_id), "Exploration", signals=signals
            ).normalized(),
        )

    def _relevance(self, context: _RecommendationContext, content: ContentProfile) -> float:
        content_terms = tokens(
            content.text + " " + " ".join(content.topics), remove_stop_words=True
        )
        if not context.user_terms or not content_terms:
            return 0.0
        return clamp_score(cosine(hashed_vector(context.user_terms), hashed_vector(content_terms)))

    def _engagement_prediction(
        self, context: _RecommendationContext, content: ContentProfile
    ) -> float:
        topic_terms = set(tokens(" ".join(content.topics), remove_stop_words=True))
        history_scores = [
            clamp_score(interaction.engagement_score)
            for interaction in context.user.history
            if interaction.content_id == content.content_id
        ]
        history_signal = sum(history_scores) / len(history_scores) if history_scores else 0.5
        complexity_fit = 1.0 - abs(clamp_score(content.complexity_level) - 0.62)
        topic_fit = 0.0
        if topic_terms:
            topic_fit = len(topic_terms & context.user_interest_terms) / len(topic_terms)
        return clamp_score(0.45 * history_signal + 0.35 * complexity_fit + 0.2 * topic_fit)

    def _temporal_score(self, content: ContentProfile, reference_time: float) -> float:
        if content.temporal is None:
            return clamp_score(content.signals.recency)
        return self.temporal.score(content.temporal, reference_time=reference_time).survival_score

    def _collaborative(self, content_id: str, context: _RecommendationContext) -> float:
        weighted_total = 0.0
        similarity_total = 0.0
        for peer in context.peer_features:
            similarity = max(0.0, cosine(context.user_vector, peer.vector))
            peer_scores = peer.history_by_content.get(content_id, ())
            if not peer_scores or similarity <= 0.0:
                continue
            weighted_total += similarity * (sum(peer_scores) / len(peer_scores))
            similarity_total += similarity
        if similarity_total == 0.0:
            return 0.5
        return clamp_score(weighted_total / similarity_total)

    def _confidence(
        self,
        user: UserProfile,
        content: ContentProfile,
        peers: tuple[UserProfile, ...],
    ) -> float:
        score = 0.42
        if user.interests:
            score += 0.14
        if user.expertise:
            score += 0.1
        if content.text and content.topics:
            score += 0.12
        if user.history:
            score += 0.1
        if peers:
            score += 0.08
        return min(0.95, score)


def _profile_at(user: UserProfile, reference_time: float) -> UserProfile:
    return replace(
        user,
        history=tuple(
            interaction for interaction in user.history
            if _valid_historical_timestamp(interaction.timestamp, reference_time)
        ),
    )


def _valid_historical_timestamp(value: object, reference_time: float) -> bool:
    return (
        not isinstance(value, bool)
        and isinstance(value, (int, float))
        and math.isfinite(value)
        and float(value) <= reference_time
    )
