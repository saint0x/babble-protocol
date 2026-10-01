"""Legacy recommendation-family adapter, not the public protocol Lens API.

This local experiment uses recency, weirdness and emerging personalization signals,
has no followed-author or timestamp fields, and cannot implement public Lens ties.
Its historical coefficients remain for callers of recommendation.py; the worker's
rank method exclusively serves ranking.py and its Rust-compatible public DTOs.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from enum import StrEnum
from typing import cast

from babble_algorithms.types import (
    Candidate,
    LensContribution,
    ObjectId,
    RankedCandidate,
    RankingTrace,
    clamp_score,
)


class BuiltInLens(StrEnum):
    FOLLOWING = "following"
    FRIENDS = "friends"
    RESEARCH = "research"
    INTELLECTUAL_SERENDIPITY = "intellectual_serendipity"
    CONTRADICTIONS = "contradictions"
    EMERGING = "emerging"
    SLOW_INTERNET = "slow_internet"
    WEIRD = "weird"


@dataclass(frozen=True, slots=True)
class LensWeight:
    lens: BuiltInLens
    weight: float

    def __post_init__(self) -> None:
        object.__setattr__(self, "lens", _built_in_lens(self.lens))
        object.__setattr__(self, "weight", _weight_value(self.weight))


@dataclass(frozen=True, slots=True)
class LensStack:
    weights: tuple[LensWeight, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "weights", _lens_weights(self.weights))

    @classmethod
    def following(cls) -> LensStack:
        return cls((LensWeight(BuiltInLens.FOLLOWING, 1.0),))

    def rank(self, candidates: tuple[Candidate, ...]) -> RankingTrace:
        weights = _normalize_weights(self.weights)
        ranked = tuple(
            sorted(
                (self._rank_candidate(candidate, weights) for candidate in _candidates(candidates)),
                key=lambda ranked_candidate: ranked_candidate.score,
                reverse=True,
            )
        )
        return RankingTrace(ranked)

    def _rank_candidate(
        self, candidate: Candidate, weights: tuple[LensWeight, ...]
    ) -> RankedCandidate:
        candidate = candidate.normalized()
        contributions = tuple(_contribution(candidate, lens_weight) for lens_weight in weights)
        score = clamp_score(sum(item.score * item.weight for item in contributions))
        return RankedCandidate(candidate=candidate, score=score, contributions=contributions)


def _built_in_lens(value: object) -> BuiltInLens:
    if not isinstance(value, BuiltInLens):
        raise ValueError("lens must be a built-in lens")
    return value


def _lens_weights(weights: object) -> tuple[LensWeight, ...]:
    if type(weights) is not tuple:
        raise ValueError("lens weights must be a tuple")
    normalized: list[LensWeight] = []
    seen: set[BuiltInLens] = set()
    for weight in cast(tuple[object, ...], weights):
        if not isinstance(weight, LensWeight):
            raise ValueError("lens weights must contain LensWeight values")
        if weight.lens in seen:
            raise ValueError(f"duplicate lens weight: {weight.lens.value}")
        seen.add(weight.lens)
        normalized.append(weight)
    return tuple(normalized)


def _candidates(candidates: object) -> tuple[Candidate, ...]:
    if type(candidates) is not tuple:
        raise ValueError("candidates must be a tuple")
    normalized: list[Candidate] = []
    seen: set[ObjectId] = set()
    for candidate in cast(tuple[object, ...], candidates):
        if not isinstance(candidate, Candidate):
            raise ValueError("candidates must contain Candidate values")
        if candidate.object_id in seen:
            raise ValueError(f"duplicate candidate object_id: {candidate.object_id}")
        seen.add(candidate.object_id)
        normalized.append(candidate)
    return tuple(normalized)


def _normalize_weights(weights: tuple[LensWeight, ...]) -> tuple[LensWeight, ...]:
    normalized = tuple(LensWeight(weight.lens, weight.weight) for weight in weights)
    valid = tuple(weight for weight in normalized if weight.weight > 0.0)
    if not valid:
        return LensStack.following().weights
    scale = max(weight.weight for weight in valid)
    total = math.fsum(weight.weight / scale for weight in valid)
    return tuple(LensWeight(weight.lens, (weight.weight / scale) / total) for weight in valid)


def _weight_value(value: object) -> float:
    if (
        isinstance(value, bool)
        or not isinstance(value, int | float)
        or not math.isfinite(value)
        or value < 0.0
    ):
        raise ValueError("lens weights must be finite and non-negative")
    return float(value)


def _contribution(candidate: Candidate, lens_weight: LensWeight) -> LensContribution:
    signals = candidate.signals
    if lens_weight.lens == BuiltInLens.FOLLOWING:
        score = clamp_score(
            0.5 * (1.0 - signals.social_distance)
            + 0.2 * signals.recency
            + 0.2 * signals.relevance
            + 0.1 * signals.reputation.following_score()
        )
        reason = "close social source, recency, relevance, and constructive reputation"
    elif lens_weight.lens == BuiltInLens.FRIENDS:
        score = clamp_score(
            0.4 * (1.0 - signals.social_distance)
            + 0.2 * signals.reputation.social_constructiveness
            + 0.2 * signals.recency
            + 0.2 * signals.relevance
        )
        reason = "social proximity, constructive reputation, recency, and relevance"
    elif lens_weight.lens == BuiltInLens.RESEARCH:
        score = clamp_score(
            0.35 * signals.evidence_quality
            + 0.20 * signals.relevance
            + 0.20 * signals.contradiction
            + 0.15 * signals.novelty
            + 0.10 * signals.reputation.research_score()
        )
        reason = "evidence quality, relevance, contradiction, novelty, and research reputation"
    elif lens_weight.lens == BuiltInLens.INTELLECTUAL_SERENDIPITY:
        adjacent_relevance = max(0.0, 1.0 - abs(signals.relevance - 0.62) / 0.62)
        score = clamp_score(
            0.25 * adjacent_relevance
            + 0.25 * signals.novelty
            + 0.18 * signals.evidence_quality
            + 0.16 * signals.weirdness
            + 0.10 * signals.reputation.research_score()
            + 0.06 * signals.recency
        )
        reason = "adjacent relevance, novelty, evidence, exploration, and research reputation"
    elif lens_weight.lens == BuiltInLens.CONTRADICTIONS:
        score = clamp_score(
            0.42 * signals.contradiction
            + 0.18 * signals.evidence.contradiction_score()
            + 0.16 * signals.evidence_quality
            + 0.14 * signals.relevance
            + 0.10 * signals.novelty
        )
        reason = "contradiction, counterevidence, evidence quality, relevance, and novelty"
    elif lens_weight.lens == BuiltInLens.EMERGING:
        score = clamp_score(
            0.28 * signals.recency
            + 0.24 * signals.emerging
            + 0.22 * signals.novelty
            + 0.14 * signals.weirdness
            + 0.12 * signals.reputation.creative_score()
        )
        reason = "recency, emerging signal, novelty, exploration, and creative reputation"
    elif lens_weight.lens == BuiltInLens.SLOW_INTERNET:
        score = clamp_score(
            0.26 * signals.evidence_quality
            + 0.20 * signals.reputation.research_score()
            + 0.18 * signals.relevance
            + 0.14 * signals.novelty
            + 0.12 * signals.contradiction
            + 0.10 * (1.0 - signals.recency)
        )
        reason = "durable evidence, research reputation, relevance, novelty, and context"
    else:
        score = clamp_score(
            0.35 * signals.weirdness
            + 0.3 * signals.novelty
            + 0.25 * signals.emerging
            + 0.1 * signals.reputation.creative_score()
        )
        reason = "weirdness, novelty, emerging signal, and creative reputation"

    return LensContribution(
        lens=lens_weight.lens.value,
        weight=lens_weight.weight,
        score=score,
        reason=reason,
    )
