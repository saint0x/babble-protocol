"""Protocol source diversity: soft floors and concentration penalties, never quotas."""

from collections import Counter

from babble_algorithms.ranking_types import (
    DiversifiedCandidateTrace,
    DiversityPolicy,
    DiversityTrace,
    RankedCandidate,
    Reason,
)
from babble_algorithms.types import CandidateSource


def diversity_reasons(
    candidate: RankedCandidate,
    counts: Counter[CandidateSource],
    selected: int,
    policy: DiversityPolicy,
) -> tuple[Reason, ...]:
    if selected == 0:
        return ()
    count = counts[candidate.candidate.source]
    reasons: list[Reason] = []
    bonus = max(
        (
            0.18 / (count + 1)
            for floor in policy.source_floors
            if floor.source == candidate.candidate.source and count < floor.minimum
        ),
        default=0.0,
    )
    if bonus != 0.0:
        reasons.append(Reason("source_floor", bonus))
    share = (count + 1) / (selected + 1)
    if share > policy.max_source_share:
        penalty = min(
            0.25,
            max(
                0.0,
                (share - policy.max_source_share) / max(0.01, 1.0 - policy.max_source_share) * 0.25,
            ),
        )
        if penalty != 0.0:
            reasons.append(Reason("source_concentration", -penalty))
    return tuple(reasons)


def diversify(
    ranked: tuple[RankedCandidate, ...],
    policy: DiversityPolicy,
    limit: int,
    times: dict[str, int],
) -> tuple[tuple[RankedCandidate, ...], DiversityTrace]:
    policy = DiversityPolicy(
        policy.max_source_share, tuple(floor for floor in policy.source_floors if floor.minimum > 0)
    )
    remaining = list(ranked)
    selected: list[RankedCandidate] = []
    traces: list[DiversifiedCandidateTrace] = []
    counts: Counter[CandidateSource] = Counter()
    while remaining and len(selected) < limit:
        adjusted: list[tuple[float, tuple[Reason, ...]]] = []
        for candidate in remaining:
            reasons = diversity_reasons(candidate, counts, len(selected), policy)
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
        policy, tuple(traces), tuple(candidate.candidate.object_id for candidate in remaining)
    )
