"""Legacy personalized feed diversity, not protocol source diversity.

Creator/topic/seen-history penalties belong only to the local recommendation API.
The public worker uses ranking_diversity.py with source-only soft adjustments.
Neither algorithm promises hard source quotas.
"""

from __future__ import annotations

import math
from collections import Counter
from dataclasses import dataclass, field

from babble_algorithms.types import (
    Candidate,
    CandidateSource,
    ObjectId,
    RankedCandidate,
    clamp_score,
)


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
            max_creator_share=_unit_score(self.max_creator_share, "max_creator_share"),
            max_topic_share=_unit_score(self.max_topic_share, "max_topic_share"),
            saturation_strength=_unit_score(self.saturation_strength, "saturation_strength"),
            source_floors=tuple(_source_floor(floor) for floor in self.source_floors),
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
        source_floor_minimums: dict[CandidateSource, int] = {}
        for floor in self.policy.source_floors:
            source_floor_minimums[floor.source] = max(
                source_floor_minimums.get(floor.source, 0), floor.minimum
            )
        self._source_floor_minimums: dict[CandidateSource, int] = source_floor_minimums

    def diversify(
        self,
        ranked: tuple[RankedCandidate, ...],
        contexts: tuple[FeedObjectContext, ...],
        *,
        limit: int | None = None,
    ) -> DiversityTrace:
        target = (
            len(ranked) if limit is None else min(_nonnegative_int(limit, "limit"), len(ranked))
        )
        contexts_by_object = _contexts_by_object(contexts)
        remaining = [_ranked_candidate(candidate) for candidate in ranked]
        selected: list[DiversifiedCandidate] = []
        state = _DiversityState()
        while remaining and len(selected) < target:
            next_candidate = max(
                remaining,
                key=lambda candidate: self._selection_key(
                    candidate, contexts_by_object.get(candidate.candidate.object_id), state
                ),
            )
            remaining.remove(next_candidate)
            context = contexts_by_object.get(next_candidate.candidate.object_id)
            diversified = self._adjust(next_candidate, context, state)
            selected.append(diversified)
            state.add(diversified)

        filtered = tuple(candidate.candidate.object_id for candidate in remaining)
        return DiversityTrace(ranked=tuple(selected), filtered=filtered)

    def _selection_key(
        self,
        ranked: RankedCandidate,
        context: FeedObjectContext | None,
        state: _DiversityState,
    ) -> tuple[float, float, str]:
        adjusted = self._adjust(ranked, context, state)
        return (
            adjusted.adjusted_score,
            ranked.score,
            str(ranked.candidate.object_id),
        )

    def _adjust(
        self,
        ranked: RankedCandidate,
        context: FeedObjectContext | None,
        state: _DiversityState,
    ) -> DiversifiedCandidate:
        reasons = [
            DiversityReason("lens_score", ranked.score),
            DiversityReason("creator_concentration", -self._creator_penalty(context, state)),
            DiversityReason("topic_saturation", -self._topic_penalty(context, state)),
            DiversityReason("seen_saturation", -self._seen_penalty(context)),
            DiversityReason("source_floor", self._source_floor_bonus(ranked, state)),
        ]
        adjusted_score = clamp_score(sum(reason.contribution for reason in reasons))
        return DiversifiedCandidate(
            ranked=ranked,
            adjusted_score=adjusted_score,
            reasons=tuple(reason for reason in reasons if reason.contribution != 0.0),
            context=context,
        )

    def _creator_penalty(self, context: FeedObjectContext | None, state: _DiversityState) -> float:
        if context is None or not context.creator_id or state.selected_count == 0:
            return 0.0
        selected_position = state.selected_count + 1
        creator_count = state.creator_counts[context.creator_id]
        next_share = (creator_count + 1) / selected_position
        if next_share <= self.policy.max_creator_share:
            return 0.0
        excess = next_share - self.policy.max_creator_share
        return clamp_score(excess / max(0.01, 1.0 - self.policy.max_creator_share)) * 0.28

    def _topic_penalty(self, context: FeedObjectContext | None, state: _DiversityState) -> float:
        if context is None or not context.topics or state.selected_count == 0:
            return 0.0
        selected_position = state.selected_count + 1
        penalty = 0.0
        for topic in context.topics:
            topic_count = state.topic_counts[topic]
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

    def _source_floor_bonus(self, ranked: RankedCandidate, state: _DiversityState) -> float:
        if state.selected_count == 0:
            return 0.0
        minimum = self._source_floor_minimums.get(ranked.candidate.source, 0)
        selected_count = state.source_counts[ranked.candidate.source]
        if minimum <= 0 or selected_count >= minimum:
            return 0.0
        return 0.18 / (selected_count + 1)


@dataclass(slots=True)
class _DiversityState:
    selected_count: int = 0
    creator_counts: Counter[str] = field(default_factory=Counter)
    topic_counts: Counter[str] = field(default_factory=Counter)
    source_counts: Counter[CandidateSource] = field(default_factory=Counter)

    def add(self, candidate: DiversifiedCandidate) -> None:
        self.selected_count += 1
        self.source_counts[candidate.ranked.candidate.source] += 1
        if candidate.context is None:
            return
        self.creator_counts[candidate.context.creator_id] += 1
        for topic in candidate.context.topics:
            self.topic_counts[topic] += 1


def _contexts_by_object(
    contexts: tuple[FeedObjectContext, ...],
) -> dict[ObjectId, FeedObjectContext]:
    by_object: dict[ObjectId, FeedObjectContext] = {}
    for context in contexts:
        normalized = _context(context)
        by_object[normalized.object_id] = normalized
    return by_object


def _context(context: FeedObjectContext) -> FeedObjectContext:
    return FeedObjectContext(
        object_id=_object_id(context.object_id),
        creator_id=_label(context.creator_id, "creator_id"),
        topics=tuple(dict.fromkeys(_topic(topic) for topic in context.topics)),
        seen_count=_nonnegative_int(context.seen_count, "seen_count"),
    )


def _ranked_candidate(ranked: RankedCandidate) -> RankedCandidate:
    score = _unit_score(ranked.score, "ranked score")
    candidate = ranked.candidate.normalized()
    source = _source(candidate.source)
    sources = tuple(
        contribution.normalized()
        for contribution in candidate.sources
        for _ in (_source(contribution.source),)
    )
    return RankedCandidate(
        Candidate(_object_id(candidate.object_id), source, sources, candidate.signals).normalized(),
        score,
        ranked.contributions,
    )


def _source_floor(floor: SourceFloor) -> SourceFloor:
    source = _source(floor.source)
    minimum = _nonnegative_int(floor.minimum, "source floor minimum")
    return SourceFloor(source, minimum)


def _source(source: CandidateSource) -> CandidateSource:
    allowed: frozenset[CandidateSource] = frozenset(
        {
            "Following",
            "SocialGraph",
            "SemanticNeighborhood",
            "Temporal",
            "Emerging",
            "Evidence",
            "Contradiction",
            "Exploration",
        }
    )
    if source not in allowed:
        raise ValueError(f"unknown candidate source: {source!r}")
    return source


def _unit_score(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float) or not math.isfinite(value):
        raise ValueError(f"{label} must be a finite number")
    return clamp_score(float(value))


def _nonnegative_int(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{label} must be a non-negative integer")
    return value


def _object_id(value: object) -> ObjectId:
    if not isinstance(value, str) or not value.strip() or any(ch.isspace() for ch in value):
        raise ValueError("object_id must be a non-empty object id without whitespace")
    return ObjectId(value)


def _label(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{label} must be a non-empty string")
    return value.strip()


def _topic(value: object) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError("topic must be a non-empty string")
    return value.strip().casefold()


def item_context_creator(item: DiversifiedCandidate) -> str:
    return item.context.creator_id if item.context else ""


def item_context_topics(item: DiversifiedCandidate) -> frozenset[str]:
    if item.context is None:
        return frozenset()
    return frozenset(item.context.topics)
