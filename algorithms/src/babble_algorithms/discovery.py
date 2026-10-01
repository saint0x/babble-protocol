from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass, field

from babble_algorithms.types import (
    Candidate,
    CandidateSource,
    CandidateSourceContribution,
    ObjectId,
    ObjectSignals,
)


@dataclass(frozen=True, slots=True)
class DiscoveryRequest:
    followed: tuple[ObjectId, ...] = ()
    anchors: tuple[ObjectId, ...] = ()
    supporting_evidence: tuple[ObjectId, ...] = ()
    contradicting_evidence: tuple[ObjectId, ...] = ()
    semantic_neighbors: tuple[ObjectId, ...] = ()
    emerging: tuple[ObjectId, ...] = ()
    object_signals: dict[ObjectId, ObjectSignals] = field(default_factory=dict)
    exploration_slots: int = 2


class CandidateEngine:
    def candidates(self, request: DiscoveryRequest) -> tuple[Candidate, ...]:
        request = _validate_request(request)
        candidates: dict[ObjectId, Candidate] = {}
        order: list[ObjectId] = []

        def push(object_id: ObjectId, source: CandidateSource, signals: ObjectSignals) -> None:
            if object_id in candidates:
                existing = candidates[object_id]
                candidates[object_id] = Candidate(
                    object_id=object_id,
                    source=_primary_source(existing.source, source),
                    sources=_merge_sources(
                        existing.sources, (CandidateSourceContribution(source),)
                    ),
                    signals=_merge_signal_objects(existing.signals, signals),
                ).normalized()
                return
            order.append(object_id)
            candidates[object_id] = Candidate(
                object_id,
                source,
                (CandidateSourceContribution(source),),
                signals.normalized(),
            )

        for object_id in request.followed:
            push(
                object_id,
                "Following",
                _merge(request, object_id, relevance=0.72, social_distance=0.1),
            )
        for object_id in request.anchors:
            push(object_id, "Temporal", _merge(request, object_id, relevance=0.9, recency=0.7))
        for object_id in request.supporting_evidence:
            push(
                object_id,
                "Evidence",
                _merge(request, object_id, relevance=0.66, evidence_quality=0.82),
            )
        for object_id in request.contradicting_evidence:
            push(
                object_id,
                "Contradiction",
                _merge(request, object_id, relevance=0.7, contradiction=0.9, novelty=0.72),
            )
        for object_id in request.semantic_neighbors:
            push(
                object_id,
                "SemanticNeighborhood",
                _merge(request, object_id, relevance=0.62, novelty=0.58),
            )
        for object_id in request.emerging:
            push(object_id, "Emerging", _merge(request, object_id, emerging=0.86, novelty=0.64))

        exploration_count = 0
        for object_id, signals in request.object_signals.items():
            if exploration_count >= request.exploration_slots:
                break
            if object_id not in candidates:
                push(object_id, "Exploration", signals)
                exploration_count += 1

        return tuple(candidates[object_id] for object_id in order)


def _validate_request(request: DiscoveryRequest) -> DiscoveryRequest:
    exploration_slots = _nonnegative_int(request.exploration_slots, "exploration_slots")
    object_signals: dict[ObjectId, ObjectSignals] = {}
    for object_id, signals in request.object_signals.items():
        object_signals[_object_id(object_id)] = signals.normalized()
    return DiscoveryRequest(
        followed=_object_ids(request.followed, "followed"),
        anchors=_object_ids(request.anchors, "anchors"),
        supporting_evidence=_object_ids(request.supporting_evidence, "supporting_evidence"),
        contradicting_evidence=_object_ids(
            request.contradicting_evidence, "contradicting_evidence"
        ),
        semantic_neighbors=_object_ids(request.semantic_neighbors, "semantic_neighbors"),
        emerging=_object_ids(request.emerging, "emerging"),
        object_signals=object_signals,
        exploration_slots=exploration_slots,
    )


def _object_ids(values: Iterable[ObjectId], label: str) -> tuple[ObjectId, ...]:
    return tuple(_object_id(value, label) for value in values)


def _object_id(value: object, label: str = "object_id") -> ObjectId:
    if not isinstance(value, str) or not value.strip() or any(ch.isspace() for ch in value):
        raise ValueError(f"{label} must be a non-empty object id without whitespace")
    return ObjectId(value)


def _nonnegative_int(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{label} must be a non-negative integer")
    return value


def _merge(request: DiscoveryRequest, object_id: ObjectId, **overrides: float) -> ObjectSignals:
    if object_id in request.object_signals:
        return request.object_signals[object_id]
    return _merge_signals(ObjectSignals(), **overrides)


def _merge_signals(signals: ObjectSignals, **overrides: float) -> ObjectSignals:
    values = {
        "relevance": signals.relevance,
        "novelty": signals.novelty,
        "evidence_quality": signals.evidence_quality,
        "contradiction": signals.contradiction,
        "social_distance": signals.social_distance,
        "emerging": signals.emerging,
        "weirdness": signals.weirdness,
        "recency": signals.recency,
    }
    values.update(overrides)
    return ObjectSignals(
        relevance=values["relevance"],
        novelty=values["novelty"],
        evidence_quality=values["evidence_quality"],
        contradiction=values["contradiction"],
        evidence=signals.evidence,
        reputation=signals.reputation,
        social_distance=values["social_distance"],
        emerging=values["emerging"],
        weirdness=values["weirdness"],
        recency=values["recency"],
    ).normalized()


def _merge_signal_objects(left: ObjectSignals, right: ObjectSignals) -> ObjectSignals:
    return ObjectSignals(
        relevance=max(left.relevance, right.relevance),
        novelty=max(left.novelty, right.novelty),
        evidence_quality=max(left.evidence_quality, right.evidence_quality),
        contradiction=max(left.contradiction, right.contradiction),
        evidence=left.evidence
        if left.evidence.support_score() >= right.evidence.support_score()
        else right.evidence,
        reputation=left.reputation
        if left.reputation.research_score() >= right.reputation.research_score()
        else right.reputation,
        social_distance=min(left.social_distance, right.social_distance),
        emerging=max(left.emerging, right.emerging),
        weirdness=max(left.weirdness, right.weirdness),
        recency=max(left.recency, right.recency),
    ).normalized()


def _merge_sources(
    left: tuple[CandidateSourceContribution, ...],
    right: tuple[CandidateSourceContribution, ...],
) -> tuple[CandidateSourceContribution, ...]:
    merged: dict[CandidateSource, float] = {}
    for contribution in left + right:
        merged[contribution.source] = max(merged.get(contribution.source, 0.0), contribution.weight)
    return tuple(
        CandidateSourceContribution(source, weight).normalized()
        for source, weight in sorted(
            merged.items(), key=lambda item: (_source_priority(item[0]), item[1]), reverse=True
        )
    )


def _primary_source(left: CandidateSource, right: CandidateSource) -> CandidateSource:
    return left if _source_priority(left) >= _source_priority(right) else right


def _source_priority(source: CandidateSource) -> int:
    priorities: dict[CandidateSource, int] = {
        "Following": 90,
        "Evidence": 80,
        "Contradiction": 75,
        "SemanticNeighborhood": 65,
        "Emerging": 55,
        "Temporal": 45,
        "SocialGraph": 40,
        "Exploration": 35,
    }
    return priorities[source]
