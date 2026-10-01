"""Legacy personalized feed diversity, not protocol source diversity.

Creator/topic/seen-history penalties belong only to the local recommendation API.
The public worker uses ranking_diversity.py with source-only soft adjustments.
Neither algorithm promises hard source quotas.
"""

from __future__ import annotations

from dataclasses import dataclass

from babble_algorithms.types import CandidateSource, ObjectId, RankedCandidate, clamp_score


@dataclass(frozen=True, slots=True)
class FeedObjectContext:
    object_id: ObjectId
    creator_id: str
    topics: tuple[str, ...] = ()
    seen_count: int = 0


@dataclass(frozen=True, slots=True)
class SourceFloor:
    source: CandidateSource
    minimum: int


@dataclass(frozen=True, slots=True)
class DiversityPolicy:
    max_creator_share: float = 0.45
    max_topic_share: float = 0.55
    saturation_strength: float = 0.32
    source_floors: tuple[SourceFloor, ...] = (
        SourceFloor("Exploration", 1),
        SourceFloor("Contradiction", 1),
    )

    def normalized(self) -> DiversityPolicy:
        return DiversityPolicy(
            max_creator_share=clamp_score(self.max_creator_share),
            max_topic_share=clamp_score(self.max_topic_share),
            saturation_strength=clamp_score(self.saturation_strength),
            source_floors=tuple(
                SourceFloor(floor.source, max(0, floor.minimum)) for floor in self.source_floors
            ),
        )


@dataclass(frozen=True, slots=True)
class DiversityReason:
    signal: str
    contribution: float


@dataclass(frozen=True, slots=True)
class DiversifiedCandidate:
    ranked: RankedCandidate
    adjusted_score: float
    reasons: tuple[DiversityReason, ...]
    context: FeedObjectContext | None = None


@dataclass(frozen=True, slots=True)
class DiversityTrace:
    ranked: tuple[DiversifiedCandidate, ...]
    filtered: tuple[ObjectId, ...]


class FeedDiversifier:
    def __init__(self, policy: DiversityPolicy | None = None) -> None:
        self.policy: DiversityPolicy = (policy or DiversityPolicy()).normalized()

    def diversify(
        self,
        ranked: tuple[RankedCandidate, ...],
        contexts: tuple[FeedObjectContext, ...],
        *,
        limit: int | None = None,
    ) -> DiversityTrace:
        target = len(ranked) if limit is None else max(0, min(limit, len(ranked)))
        contexts_by_object = {context.object_id: context for context in contexts}
        remaining = list(ranked)
        selected: list[DiversifiedCandidate] = []
        while remaining and len(selected) < target:
            next_candidate = max(
                remaining,
                key=lambda candidate: self._selection_key(
                    candidate, contexts_by_object.get(candidate.candidate.object_id), selected
                ),
            )
            remaining.remove(next_candidate)
            context = contexts_by_object.get(next_candidate.candidate.object_id)
            selected.append(self._adjust(next_candidate, context, selected))

        filtered = tuple(candidate.candidate.object_id for candidate in remaining)
        return DiversityTrace(ranked=tuple(selected), filtered=filtered)

    def _selection_key(
        self,
        ranked: RankedCandidate,
        context: FeedObjectContext | None,
        selected: list[DiversifiedCandidate],
    ) -> tuple[float, float, str]:
        adjusted = self._adjust(ranked, context, selected)
        return (
            adjusted.adjusted_score,
            ranked.score,
            str(ranked.candidate.object_id),
        )

    def _adjust(
        self,
        ranked: RankedCandidate,
        context: FeedObjectContext | None,
        selected: list[DiversifiedCandidate],
    ) -> DiversifiedCandidate:
        reasons = [
            DiversityReason("lens_score", ranked.score),
            DiversityReason("creator_concentration", -self._creator_penalty(context, selected)),
            DiversityReason("topic_saturation", -self._topic_penalty(context, selected)),
            DiversityReason("seen_saturation", -self._seen_penalty(context)),
            DiversityReason("source_floor", self._source_floor_bonus(ranked, selected)),
        ]
        adjusted_score = clamp_score(sum(reason.contribution for reason in reasons))
        return DiversifiedCandidate(
            ranked=ranked,
            adjusted_score=adjusted_score,
            reasons=tuple(reason for reason in reasons if reason.contribution != 0.0),
            context=context,
        )

    def _creator_penalty(
        self, context: FeedObjectContext | None, selected: list[DiversifiedCandidate]
    ) -> float:
        if context is None or not context.creator_id or not selected:
            return 0.0
        selected_position = len(selected) + 1
        creator_count = sum(
            1
            for item in selected
            if item_context_creator(item) == context.creator_id
        )
        next_share = (creator_count + 1) / selected_position
        if next_share <= self.policy.max_creator_share:
            return 0.0
        excess = next_share - self.policy.max_creator_share
        return clamp_score(excess / max(0.01, 1.0 - self.policy.max_creator_share)) * 0.28

    def _topic_penalty(
        self, context: FeedObjectContext | None, selected: list[DiversifiedCandidate]
    ) -> float:
        if context is None or not context.topics or not selected:
            return 0.0
        selected_position = len(selected) + 1
        penalty = 0.0
        for topic in {topic.lower() for topic in context.topics if topic}:
            topic_count = sum(1 for item in selected if topic in item_context_topics(item))
            next_share = (topic_count + 1) / selected_position
            if next_share > self.policy.max_topic_share:
                excess = next_share - self.policy.max_topic_share
                penalty = max(
                    penalty,
                    clamp_score(excess / max(0.01, 1.0 - self.policy.max_topic_share)) * 0.22,
                )
        return penalty

    def _seen_penalty(self, context: FeedObjectContext | None) -> float:
        if context is None or context.seen_count <= 0:
            return 0.0
        saturation = context.seen_count / (context.seen_count + 4.0)
        return clamp_score(saturation * self.policy.saturation_strength)

    def _source_floor_bonus(
        self, ranked: RankedCandidate, selected: list[DiversifiedCandidate]
    ) -> float:
        if not selected:
            return 0.0
        bonus = 0.0
        for floor in self.policy.source_floors:
            if ranked.candidate.source != floor.source or floor.minimum <= 0:
                continue
            selected_count = sum(
                1 for item in selected if item.ranked.candidate.source == floor.source
            )
            if selected_count < floor.minimum:
                bonus = max(bonus, 0.18 / (selected_count + 1))
        return bonus


def item_context_creator(item: DiversifiedCandidate) -> str:
    return item.context.creator_id if item.context else ""


def item_context_topics(item: DiversifiedCandidate) -> frozenset[str]:
    if item.context is None:
        return frozenset()
    return frozenset(topic.lower() for topic in item.context.topics if topic)
