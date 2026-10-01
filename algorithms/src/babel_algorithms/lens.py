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

from babel_algorithms.types import (
    Candidate,
    LensContribution,
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


@dataclass(frozen=True, slots=True)
class LensStack:
    weights: tuple[LensWeight, ...]

    @classmethod
    def following(cls) -> LensStack:
        return cls((LensWeight(BuiltInLens.FOLLOWING, 1.0),))

    def rank(self, candidates: tuple[Candidate, ...]) -> RankingTrace:
        weights = _normalize_weights(self.weights)
        ranked = tuple(
            sorted(
                (self._rank_candidate(candidate, weights) for candidate in candidates),
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


def _normalize_weights(weights: tuple[LensWeight, ...]) -> tuple[LensWeight, ...]:
    if any(not math.isfinite(weight.weight) or weight.weight < 0.0 for weight in weights):
        raise ValueError("lens weights must be finite and non-negative")
    valid = tuple(weight for weight in weights if weight.weight > 0.0)
    if not valid:
        return LensStack.following().weights
    scale = max(weight.weight for weight in valid)
    total = math.fsum(weight.weight / scale for weight in valid)
    return tuple(LensWeight(weight.lens, (weight.weight / scale) / total) for weight in valid)


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
