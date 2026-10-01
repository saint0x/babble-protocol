"""Canonical public Lens scoring and traces, matching backend/crates/lens."""

import math

from babble_algorithms.ranking_diversity import diversify
from babble_algorithms.ranking_time import timestamp_nanos
from babble_algorithms.ranking_types import (
    BuiltInLens,
    Candidate,
    CandidateTrace,
    DiversityPolicy,
    EvidenceSignals,
    LensContribution,
    LensStack,
    LensWeight,
    RankedCandidate,
    RankingRequest,
    RankingResult,
    RankingTrace,
    Reason,
    Signals,
    SourceFloor,
)
from babble_algorithms.types import CandidateSource, CandidateSourceContribution, ReputationSignals

_ALLOWED_SOURCES: tuple[CandidateSource, ...] = (
    "Following",
    "SocialGraph",
    "SemanticNeighborhood",
    "Temporal",
    "Emerging",
    "Evidence",
    "Contradiction",
    "Exploration",
)


def _unit_signal(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise ValueError(f"{label} must be a finite unit score")
    result = float(value)
    if not math.isfinite(result) or not 0.0 <= result <= 1.0:
        raise ValueError(f"{label} must be a finite unit score")
    return result


def _nonnegative_signal(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise ValueError(f"{label} must be finite and non-negative")
    result = float(value)
    if not math.isfinite(result) or result < 0.0:
        raise ValueError(f"{label} must be finite and non-negative")
    return result


def _exact_count(value: object, label: str, maximum: int = 200) -> int:
    if type(value) is not int or not 0 <= value <= maximum:
        raise ValueError(f"{label} must be a bounded integer")
    return value


def _validate_source(value: object, label: str) -> CandidateSource:
    if value not in _ALLOWED_SOURCES:
        raise ValueError(f"{label} must be a supported source")
    return value


def _validate_sources(candidate: Candidate) -> None:
    if type(candidate.sources) is not tuple or not candidate.sources:
        raise ValueError("candidate sources must be a nonempty tuple")
    names: set[CandidateSource] = set()
    for contribution in candidate.sources:
        if type(contribution) is not CandidateSourceContribution:
            raise ValueError("candidate source contributions must be typed")
        source = _validate_source(contribution.source, "candidate source")
        if source in names:
            raise ValueError("candidate source contributions must be unique")
        names.add(source)
        _ = _unit_signal(contribution.weight, "candidate source weight")
    if candidate.source not in names:
        raise ValueError("candidate sources must include the primary source")


def _validate_reputation(value: object) -> ReputationSignals:
    if not isinstance(value, ReputationSignals):
        raise ValueError("ranking reputation signals must be typed")
    for label, signal in (
        ("reputation epistemic_accuracy", value.epistemic_accuracy),
        ("reputation evidence_quality", value.evidence_quality),
        ("reputation social_constructiveness", value.social_constructiveness),
        ("reputation creative_contribution", value.creative_contribution),
        ("reputation moderation", value.moderation),
        ("reputation domain_expertise", value.domain_expertise),
    ):
        _ = _unit_signal(signal, label)
    return value


def _validate_evidence(value: object) -> EvidenceSignals:
    if not isinstance(value, EvidenceSignals):
        raise ValueError("ranking evidence signals must be typed")
    for label, signal in (
        ("evidence human_support", value.human_support),
        ("evidence judgment_support", value.judgment_support),
        ("evidence human_contradiction", value.human_contradiction),
        ("evidence judgment_contradiction", value.judgment_contradiction),
    ):
        _ = _nonnegative_signal(signal, label)
    return value


def _validate_signals(value: object) -> Signals:
    if not isinstance(value, Signals):
        raise ValueError("ranking signals must be typed")
    if type(value.followed_author) is not bool:
        raise ValueError("followed_author must be a boolean")
    for label, signal in (
        ("social_distance", value.social_distance),
        ("relevance", value.relevance),
        ("novelty", value.novelty),
        ("evidence_quality", value.evidence_quality),
        ("contradiction", value.contradiction),
        ("temporal", value.temporal),
        ("exploration", value.exploration),
    ):
        _ = _unit_signal(signal, label)
    _ = _validate_evidence(value.evidence)
    _ = _validate_reputation(value.reputation)
    return value


def _validate_candidate(candidate: object) -> Candidate:
    if not isinstance(candidate, Candidate):
        raise ValueError("ranking candidates must be typed")
    if type(candidate.object_id) is not str or not candidate.object_id:
        raise ValueError("ranking candidate object_id must be a nonempty string")
    _ = _validate_source(candidate.source, "candidate primary source")
    _validate_sources(candidate)
    _ = timestamp_nanos(candidate.created_at)
    _ = _validate_signals(candidate.signals)
    return candidate


def _validate_request(request: object) -> tuple[Candidate, ...]:
    if not isinstance(request, RankingRequest):
        raise ValueError("ranking request must be typed")
    if type(request.candidates) is not tuple:
        raise ValueError("ranking candidates must be a tuple")
    if type(request.lens) is not LensStack:
        raise ValueError("ranking lens stack must be typed")
    if type(request.lens.id) is not str or not request.lens.id:
        raise ValueError("ranking lens stack ID must be a nonempty string")
    if type(request.lens.weights) is not tuple:
        raise ValueError("ranking lens weights must be a tuple")
    lens_ids: set[BuiltInLens] = set()
    for weight in request.lens.weights:
        if type(weight) is not LensWeight or type(weight.lens) is not BuiltInLens:
            raise ValueError("ranking lens weights must be typed")
        if weight.lens in lens_ids:
            raise ValueError("ranking lens weights must be unique")
        lens_ids.add(weight.lens)
        _ = _weight_value(weight.weight)
    if type(request.diversity) is not DiversityPolicy:
        raise ValueError("ranking diversity policy must be typed")
    _ = _unit_signal(request.diversity.max_source_share, "diversity max_source_share")
    if type(request.diversity.source_floors) is not tuple:
        raise ValueError("diversity source floors must be a tuple")
    floor_sources: set[CandidateSource] = set()
    for floor in request.diversity.source_floors:
        if type(floor) is not SourceFloor:
            raise ValueError("diversity source floors must be typed")
        source = _validate_source(floor.source, "diversity source floor")
        if source in floor_sources:
            raise ValueError("diversity source floors must be unique")
        floor_sources.add(source)
        _ = _exact_count(floor.minimum, "diversity source floor")
    _ = _exact_count(request.limit, "ranking limit")
    candidates = tuple(_validate_candidate(candidate) for candidate in request.candidates)
    if len({candidate.object_id for candidate in candidates}) != len(candidates):
        raise ValueError("ranking candidates must have unique IDs")
    return candidates


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
    candidates = _validate_request(request)
    weights = normalized_weights(request.lens.weights)
    ranked: list[RankedCandidate] = []
    contributions: dict[str, tuple[LensContribution, ...]] = {}
    times = {candidate.object_id: timestamp_nanos(candidate.created_at) for candidate in candidates}
    for candidate in candidates:
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
