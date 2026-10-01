from __future__ import annotations

import math
from dataclasses import dataclass, field, replace

from babel_algorithms.temporal import TemporalInput, TemporalScorer
from babel_algorithms.text import cosine, hashed_vector, tokens
from babel_algorithms.types import Candidate, ObjectId, ObjectSignals, clamp_score


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
        for value in (self.relevance, self.engagement, self.authenticity):
            if value is not None and (not math.isfinite(value) or not 0.0 <= value <= 1.0):
                raise ValueError("feedback ratings must be finite and between zero and one")


@dataclass(frozen=True, slots=True)
class RecommendationWeights:
    relevance: float = 0.3
    engagement: float = 0.2
    authenticity: float = 0.2
    temporal: float = 0.15
    collaborative: float = 0.15

    def normalized(self) -> RecommendationWeights:
        values = (
            self.relevance, self.engagement, self.authenticity, self.temporal, self.collaborative
        )
        if any(not math.isfinite(value) or value < 0.0 for value in values):
            raise ValueError("recommendation weights must be finite and non-negative")
        scale = max(values)
        if scale == 0.0:
            return RecommendationWeights()
        total = math.fsum(value / scale for value in values)
        return RecommendationWeights(
            relevance=(self.relevance / scale) / total,
            engagement=(self.engagement / scale) / total,
            authenticity=(self.authenticity / scale) / total,
            temporal=(self.temporal / scale) / total,
            collaborative=(self.collaborative / scale) / total,
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
        if not math.isfinite(learning_rate) or not 0.0 <= learning_rate <= 1.0:
            raise ValueError("learning_rate must be finite and between zero and one")
        base = self.normalized()

        def adjusted(weight: float, ratings: tuple[float | None, ...]) -> float:
            present = tuple(rating for rating in ratings if rating is not None)
            if not present:
                return weight
            delta = learning_rate * math.fsum(rating - 0.5 for rating in present) / len(present)
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


class RecommendationEngine:
    def __init__(self, weights: RecommendationWeights | None = None) -> None:
        self.weights = (weights or RecommendationWeights()).normalized()
        self.temporal = TemporalScorer()

    def recommend(
        self,
        user: UserProfile,
        content: tuple[ContentProfile, ...],
        *,
        peers: tuple[UserProfile, ...] = (),
        reference_time: float,
        limit: int | None = None,
    ) -> tuple[RecommendationScore, ...]:
        if not math.isfinite(reference_time):
            raise ValueError("reference_time must be finite")
        user = _profile_at(user, reference_time)
        peers = tuple(_profile_at(peer, reference_time) for peer in peers)
        ranked = tuple(
            sorted(
                (
                    self._score(user, item, peers=peers, reference_time=reference_time)
                    for item in content
                ),
                key=lambda score: score.final_score,
                reverse=True,
            )
        )
        if limit is None:
            return ranked
        return ranked[: max(0, limit)]

    def _score(
        self,
        user: UserProfile,
        content: ContentProfile,
        *,
        peers: tuple[UserProfile, ...],
        reference_time: float,
    ) -> RecommendationScore:
        relevance = self._relevance(user, content)
        engagement = self._engagement_prediction(user, content)
        authenticity = clamp_score(content.authenticity_score)
        temporal = self._temporal_score(content, reference_time)
        collaborative = self._collaborative(user, content.content_id, peers)
        final = clamp_score(
            self.weights.relevance * relevance
            + self.weights.engagement * engagement
            + self.weights.authenticity * authenticity
            + self.weights.temporal * temporal
            + self.weights.collaborative * collaborative
        )
        confidence = self._confidence(user, content, peers)
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

    def _relevance(self, user: UserProfile, content: ContentProfile) -> float:
        user_terms = tokens(" ".join(user.interests + user.expertise), remove_stop_words=True)
        content_terms = tokens(
            content.text + " " + " ".join(content.topics), remove_stop_words=True
        )
        if not user_terms or not content_terms:
            return 0.0
        return clamp_score(cosine(hashed_vector(user_terms), hashed_vector(content_terms)))

    def _engagement_prediction(self, user: UserProfile, content: ContentProfile) -> float:
        topic_terms = set(tokens(" ".join(content.topics), remove_stop_words=True))
        history_scores = [
            clamp_score(interaction.engagement_score)
            for interaction in user.history
            if interaction.content_id == content.content_id
        ]
        history_signal = sum(history_scores) / len(history_scores) if history_scores else 0.5
        complexity_fit = 1.0 - abs(clamp_score(content.complexity_level) - 0.62)
        topic_fit = 0.0
        if topic_terms:
            interest_terms = set(tokens(" ".join(user.interests), remove_stop_words=True))
            topic_fit = len(topic_terms & interest_terms) / len(topic_terms)
        return clamp_score(0.45 * history_signal + 0.35 * complexity_fit + 0.2 * topic_fit)

    def _temporal_score(self, content: ContentProfile, reference_time: float) -> float:
        if content.temporal is None:
            return clamp_score(content.signals.recency)
        return self.temporal.score(content.temporal, reference_time=reference_time).survival_score

    def _collaborative(
        self,
        user: UserProfile,
        content_id: str,
        peers: tuple[UserProfile, ...],
    ) -> float:
        weighted_total = 0.0
        similarity_total = 0.0
        user_vector = hashed_vector(tokens(" ".join(user.interests + user.expertise)))
        for peer in peers:
            if peer.user_id == user.user_id:
                continue
            peer_vector = hashed_vector(tokens(" ".join(peer.interests + peer.expertise)))
            similarity = max(0.0, cosine(user_vector, peer_vector))
            peer_scores = [
                clamp_score(interaction.engagement_score)
                for interaction in peer.history
                if interaction.content_id == content_id
            ]
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
            if math.isfinite(interaction.timestamp) and interaction.timestamp <= reference_time
        ),
    )
