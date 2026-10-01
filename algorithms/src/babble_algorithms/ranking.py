"""Canonical public Lens scoring and traces, matching backend/crates/lens."""

import math

from babble_algorithms.ranking_diversity import diversify
from babble_algorithms.ranking_time import timestamp_nanos
from babble_algorithms.ranking_types import (
    BuiltInLens,
    Candidate,
    CandidateTrace,
    LensContribution,
    LensWeight,
    RankedCandidate,
    RankingRequest,
    RankingResult,
    RankingTrace,
    Reason,
)
from babble_algorithms.types import CandidateSource


def normalized_weights(weights: tuple[LensWeight, ...]) -> tuple[LensWeight, ...]:
    normalized = tuple(LensWeight(weight.lens, _weight_value(weight.weight)) for weight in weights)
    positive = tuple(weight for weight in normalized if weight.weight > 0.0)
    if not positive:
        return (LensWeight(BuiltInLens.FOLLOWING, 1.0),)
    total = sequential_sum(tuple(weight.weight for weight in positive))
    if total != float("inf"):
        return tuple(LensWeight(weight.lens, weight.weight / total) for weight in positive)
    scale = max(weight.weight for weight in positive)
    total = sequential_sum(tuple(weight.weight / scale for weight in positive))
    return tuple(LensWeight(weight.lens, (weight.weight / scale) / total) for weight in positive)


def _weight_value(value: object) -> float:
    if (
        isinstance(value, bool)
        or not isinstance(value, int | float)
        or not math.isfinite(value)
        or value < 0.0
    ):
        raise ValueError("lens weights must be finite and non-negative")
    return float(value)


def source_bonus(candidate: Candidate, expected: CandidateSource, bonus: float) -> float:
    return bonus if any(source.source == expected for source in candidate.sources) else 0.0


def lens_reasons(candidate: Candidate, lens: BuiltInLens) -> tuple[Reason, ...]:
    s = candidate.signals
    r = s.reputation
    match lens:
        case BuiltInLens.FOLLOWING:
            return (
                Reason("followed_author", 0.55 if s.followed_author else 0.0),
                Reason("temporal", 0.30 * s.temporal),
                Reason("reputation.social_constructiveness", 0.07 * r.social_constructiveness),
                Reason("reputation.creative_contribution", 0.03 * r.creative_contribution),
                Reason("relevance", 0.05 * s.relevance),
            )
        case BuiltInLens.FRIENDS:
            return (
                Reason("social_distance", 0.40 * (1.0 - s.social_distance)),
                Reason("followed_author", 0.22 if s.followed_author else 0.0),
                Reason("temporal", 0.15 * s.temporal),
                Reason("reputation.social_constructiveness", 0.13 * r.social_constructiveness),
                Reason("relevance", 0.10 * s.relevance),
            )
        case BuiltInLens.RESEARCH:
            return (
                Reason("evidence_quality", 0.40 * s.evidence_quality),
                Reason("relevance", 0.25 * s.relevance),
                Reason("contradiction", 0.15 * s.contradiction),
                Reason("reputation.evidence_quality", 0.06 * r.evidence_quality),
                Reason("reputation.domain_expertise", 0.05 * r.domain_expertise),
                Reason("reputation.epistemic_accuracy", 0.04 * r.epistemic_accuracy),
                Reason("temporal", 0.05 * s.temporal),
            )
        case BuiltInLens.INTELLECTUAL_SERENDIPITY:
            adjacent = max(0.0, min(1.0, 1.0 - abs(s.relevance - 0.62) / 0.62))
            return (
                Reason("adjacent_relevance", 0.25 * adjacent),
                Reason("novelty", 0.25 * s.novelty),
                Reason("evidence_quality", 0.18 * s.evidence_quality),
                Reason("exploration", 0.16 * s.exploration),
                Reason("reputation.research", 0.10 * r.research_score()),
                Reason("temporal", 0.06 * s.temporal),
            )
        case BuiltInLens.CONTRADICTIONS:
            return (
                Reason("contradiction", 0.42 * s.contradiction),
                Reason("evidence.contradiction_score", 0.18 * s.evidence.contradiction_score()),
                Reason("evidence_quality", 0.16 * s.evidence_quality),
                Reason("relevance", 0.14 * s.relevance),
                Reason("novelty", 0.06 * s.novelty),
                Reason("source.contradiction", source_bonus(candidate, "Contradiction", 0.04)),
            )
        case BuiltInLens.EMERGING:
            return (
                Reason("temporal", 0.28 * s.temporal),
                Reason("novelty", 0.22 * s.novelty),
                Reason("exploration", 0.18 * s.exploration),
                Reason("reputation.creative_contribution", 0.12 * r.creative_contribution),
                Reason("social_distance", 0.10 * s.social_distance),
                Reason("source.emerging", source_bonus(candidate, "Emerging", 0.10)),
            )
        case BuiltInLens.SLOW_INTERNET:
            return (
                Reason("evidence_quality", 0.26 * s.evidence_quality),
                Reason("reputation.research", 0.20 * r.research_score()),
                Reason("relevance", 0.18 * s.relevance),
                Reason("novelty", 0.14 * s.novelty),
                Reason("contradiction_context", 0.12 * s.contradiction),
                Reason("durability", 0.10 * (1.0 - s.temporal)),
            )
        case BuiltInLens.WEIRD:
            return (
                Reason("novelty", 0.35 * s.novelty),
                Reason("exploration", 0.30 * s.exploration),
                Reason("semantic_distance", 0.20 * (1.0 - s.relevance)),
                Reason("reputation.creative_contribution", 0.05 * r.creative_contribution),
                Reason("emerging", source_bonus(candidate, "Emerging", 0.10)),
                Reason(
                    "contradiction",
                    source_bonus(candidate, "Contradiction", 0.05) + 0.05 * s.contradiction,
                ),
            )


def sequential_sum(values: tuple[float, ...]) -> float:
    # Python 3.12+ sum uses compensated summation; Rust f64::sum is left-to-right.
    result = 0.0
    for value in values:
        result += value
    return result


def rank(request: RankingRequest) -> RankingResult:
    weights = normalized_weights(request.lens.weights)
    ranked: list[RankedCandidate] = []
    contributions: dict[str, tuple[LensContribution, ...]] = {}
    times = {
        candidate.object_id: timestamp_nanos(candidate.created_at)
        for candidate in request.candidates
    }
    for candidate in request.candidates:
        reasons: list[Reason] = []
        lenses: list[LensContribution] = []
        score = 0.0
        for weight in weights:
            raw = lens_reasons(candidate, weight.lens)
            weighted_score = sequential_sum(tuple(reason.contribution for reason in raw))
            weighted_score *= weight.weight
            weighted = tuple(
                Reason(reason.signal, reason.contribution * weight.weight) for reason in raw
            )
            reasons.extend(
                Reason(f"{weight.lens.id}:{reason.signal}", reason.contribution)
                for reason in weighted
            )
            lenses.append(LensContribution(weight.lens.id, weight.weight, weighted_score, weighted))
            score += weighted_score
        ranked.append(RankedCandidate(candidate, score, tuple(reasons)))
        contributions[candidate.object_id] = tuple(lenses)
    ranked.sort(
        key=lambda item: (-item.score, -times[item.candidate.object_id], item.candidate.object_id)
    )
    trace = RankingTrace(
        request.lens.id,
        tuple(
            CandidateTrace(
                index,
                item.candidate.object_id,
                item.candidate.source,
                item.candidate.sources,
                item.score,
                contributions[item.candidate.object_id],
            )
            for index, item in enumerate(ranked, 1)
        ),
    )
    selected, diversity_trace = diversify(tuple(ranked), request.diversity, request.limit, times)
    return RankingResult(selected, trace, diversity_trace)
