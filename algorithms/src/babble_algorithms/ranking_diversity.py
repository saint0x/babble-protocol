"""Protocol source diversity: soft floors and concentration penalties, never quotas."""

from collections import Counter
from dataclasses import dataclass

from babble_algorithms.ranking_types import (
    DiversifiedCandidateTrace,
    DiversityPolicy,
    DiversityTrace,
    RankedCandidate,
    Reason,
    SourceFloor,
)
from babble_algorithms.types import CandidateSource


@dataclass(frozen=True, slots=True)
class _PreparedDiversityPolicy:
    policy: DiversityPolicy
    source_floors: dict[CandidateSource, int]


def _prepare_policy(policy: DiversityPolicy) -> _PreparedDiversityPolicy:
    source_floors: dict[CandidateSource, int] = {}
    for floor in policy.source_floors:
        if floor.minimum > 0:
            source_floors[floor.source] = floor.minimum
    normalized = DiversityPolicy(
        policy.max_source_share,
        tuple(SourceFloor(source, source_floors[source]) for source in source_floors),
    )
    return _PreparedDiversityPolicy(normalized, source_floors)


def diversity_reasons(
    candidate: RankedCandidate,
    counts: Counter[CandidateSource],
    selected: int,
    policy: _PreparedDiversityPolicy,
) -> tuple[Reason, ...]:
    if selected == 0:
        return ()
    count = counts[candidate.candidate.source]
    reasons: list[Reason] = []
    floor = policy.source_floors.get(candidate.candidate.source, 0)
    if count < floor:
        reasons.append(Reason("source_floor", 0.18 / (count + 1)))
    share = (count + 1) / (selected + 1)
    if share > policy.policy.max_source_share:
        penalty = min(
            0.25,
            max(
                0.0,
                (share - policy.policy.max_source_share)
                / max(0.01, 1.0 - policy.policy.max_source_share)
                * 0.25,
            ),
        )
        if penalty != 0.0:
            reasons.append(Reason("source_concentration", -penalty))
    return tuple(reasons)


def _neutral_diversity(
    ranked: tuple[RankedCandidate, ...],
    policy: DiversityPolicy,
    limit: int,
) -> tuple[tuple[RankedCandidate, ...], DiversityTrace]:
    selected: list[RankedCandidate] = []
    traces: list[DiversifiedCandidateTrace] = []
    for index, candidate in enumerate(ranked[:limit], 1):
        score = min(1.0, candidate.score)
        selected.append(RankedCandidate(candidate.candidate, score, candidate.reasons))
        traces.append(
            DiversifiedCandidateTrace(
                index,
                candidate.candidate.object_id,
                candidate.candidate.source,
                candidate.score,
                score,
                (),
            )
        )
    return tuple(selected), DiversityTrace(
        policy,
        tuple(traces),
        tuple(candidate.candidate.object_id for candidate in ranked[limit:]),
    )


def diversify(
    ranked: tuple[RankedCandidate, ...],
    policy: DiversityPolicy,
    limit: int,
    times: dict[str, int],
) -> tuple[tuple[RankedCandidate, ...], DiversityTrace]:
    prepared = _prepare_policy(policy)
    if not prepared.source_floors and prepared.policy.max_source_share >= 1.0:
        return _neutral_diversity(ranked, prepared.policy, limit)
    remaining = list(ranked)
    selected: list[RankedCandidate] = []
    traces: list[DiversifiedCandidateTrace] = []
    counts: Counter[CandidateSource] = Counter()
    while remaining and len(selected) < limit:
        adjusted: list[tuple[float, tuple[Reason, ...]]] = []
        for candidate in remaining:
            reasons = diversity_reasons(candidate, counts, len(selected), prepared)
            delta = 0.0
            for reason in reasons:
                delta += reason.contribution
            adjusted.append((max(0.0, min(1.0, candidate.score + delta)), reasons))
        index = min(
            range(len(remaining)),
            key=lambda i: (
                -adjusted[i][0],
                -remaining[i].score,
                -times[remaining[i].candidate.object_id],
                remaining[i].candidate.object_id,
            ),
        )
        candidate = remaining.pop(index)
        score, reasons = adjusted[index]
        selected.append(
            RankedCandidate(
                candidate.candidate,
                score,
                candidate.reasons
                + tuple(
                    Reason(f"diversity:{reason.signal}", reason.contribution) for reason in reasons
                ),
            )
        )
        traces.append(
            DiversifiedCandidateTrace(
                len(selected),
                candidate.candidate.object_id,
                candidate.candidate.source,
                candidate.score,
                score,
                reasons,
            )
        )
        counts[candidate.candidate.source] += 1
    return tuple(selected), DiversityTrace(
        prepared.policy,
        tuple(traces),
        tuple(candidate.candidate.object_id for candidate in remaining),
    )
