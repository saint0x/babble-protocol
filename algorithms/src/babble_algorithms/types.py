from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Literal, NewType, cast

ObjectId = NewType("ObjectId", str)
IdentityId = NewType("IdentityId", str)

CandidateSource = Literal[
    "Following",
    "SocialGraph",
    "SemanticNeighborhood",
    "Temporal",
    "Emerging",
    "Evidence",
    "Contradiction",
    "Exploration",
]

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


def clamp_score(value: float) -> float:
    if isinstance(value, bool) or not math.isfinite(value):
        return 0.0
    return max(0.0, min(1.0, value))


def nonnegative_signal(value: float) -> float:
    if isinstance(value, bool) or not math.isfinite(value):
        return 0.0
    return max(0.0, value)


def _source(value: object, label: str = "candidate source") -> CandidateSource:
    if value not in _ALLOWED_SOURCES:
        raise ValueError(f"unknown {label}: {value!r}")
    return value


def _object_id(value: object, label: str = "candidate object_id") -> ObjectId:
    if not isinstance(value, str) or not value.strip() or any(ch.isspace() for ch in value):
        raise ValueError(f"{label} must be a non-empty object id without whitespace")
    return ObjectId(value)


def _finite_number(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float) or not math.isfinite(value):
        raise ValueError(f"{label} must be a finite number")
    return float(value)


def _unit_number(value: object, label: str) -> float:
    result = _finite_number(value, label)
    if not 0.0 <= result <= 1.0:
        raise ValueError(f"{label} must be between 0 and 1")
    return result


def _label(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{label} must be a non-empty string")
    return value


@dataclass(frozen=True, slots=True)
class ReputationSignals:
    epistemic_accuracy: float = 0.0
    evidence_quality: float = 0.0
    social_constructiveness: float = 0.0
    creative_contribution: float = 0.0
    moderation: float = 0.0
    domain_expertise: float = 0.0

    def __post_init__(self) -> None:
        for field_name, value in (
            ("epistemic_accuracy", self.epistemic_accuracy),
            ("evidence_quality", self.evidence_quality),
            ("social_constructiveness", self.social_constructiveness),
            ("creative_contribution", self.creative_contribution),
            ("moderation", self.moderation),
            ("domain_expertise", self.domain_expertise),
        ):
            object.__setattr__(self, field_name, _finite_number(value, f"reputation {field_name}"))

    def normalized(self) -> ReputationSignals:
        return ReputationSignals(
            epistemic_accuracy=clamp_score(self.epistemic_accuracy),
            evidence_quality=clamp_score(self.evidence_quality),
            social_constructiveness=clamp_score(self.social_constructiveness),
            creative_contribution=clamp_score(self.creative_contribution),
            moderation=clamp_score(self.moderation),
            domain_expertise=clamp_score(self.domain_expertise),
        )

    def following_score(self) -> float:
        normalized = self.normalized()
        return clamp_score(
            0.7 * normalized.social_constructiveness + 0.3 * normalized.creative_contribution
        )

    def research_score(self) -> float:
        normalized = self.normalized()
        return clamp_score(
            0.35 * normalized.evidence_quality
            + 0.30 * normalized.domain_expertise
            + 0.25 * normalized.epistemic_accuracy
            + 0.10 * normalized.moderation
        )

    def creative_score(self) -> float:
        normalized = self.normalized()
        return clamp_score(
            0.65 * normalized.creative_contribution + 0.35 * normalized.social_constructiveness
        )


@dataclass(frozen=True, slots=True)
class EvidenceSignals:
    human_support: float = 0.0
    judgment_support: float = 0.0
    human_contradiction: float = 0.0
    judgment_contradiction: float = 0.0

    def __post_init__(self) -> None:
        for field_name, value in (
            ("human_support", self.human_support),
            ("judgment_support", self.judgment_support),
            ("human_contradiction", self.human_contradiction),
            ("judgment_contradiction", self.judgment_contradiction),
        ):
            object.__setattr__(self, field_name, _finite_number(value, f"evidence {field_name}"))

    def normalized(self) -> EvidenceSignals:
        return EvidenceSignals(
            human_support=nonnegative_signal(self.human_support),
            judgment_support=nonnegative_signal(self.judgment_support),
            human_contradiction=nonnegative_signal(self.human_contradiction),
            judgment_contradiction=nonnegative_signal(self.judgment_contradiction),
        )

    def support_score(self) -> float:
        normalized = self.normalized()
        return clamp_score((normalized.human_support + 0.75 * normalized.judgment_support) / 3.0)

    def contradiction_score(self) -> float:
        normalized = self.normalized()
        return clamp_score(
            (normalized.human_contradiction + 0.75 * normalized.judgment_contradiction) / 3.0
        )


@dataclass(frozen=True, slots=True)
class ObjectSignals:
    relevance: float = 0.5
    novelty: float = 0.5
    evidence_quality: float = 0.5
    contradiction: float = 0.0
    evidence: EvidenceSignals = field(default_factory=EvidenceSignals)
    reputation: ReputationSignals = field(default_factory=ReputationSignals)
    social_distance: float = 1.0
    emerging: float = 0.0
    weirdness: float = 0.0
    recency: float = 0.5

    def __post_init__(self) -> None:
        if type(self.evidence) is not EvidenceSignals:
            raise ValueError("evidence must be EvidenceSignals")
        if type(self.reputation) is not ReputationSignals:
            raise ValueError("reputation must be ReputationSignals")

    def normalized(self) -> ObjectSignals:
        return ObjectSignals(
            relevance=clamp_score(self.relevance),
            novelty=clamp_score(self.novelty),
            evidence_quality=clamp_score(self.evidence_quality),
            contradiction=clamp_score(self.contradiction),
            evidence=self.evidence.normalized(),
            reputation=self.reputation.normalized(),
            social_distance=clamp_score(self.social_distance),
            emerging=clamp_score(self.emerging),
            weirdness=clamp_score(self.weirdness),
            recency=clamp_score(self.recency),
        )


@dataclass(frozen=True, slots=True)
class CandidateSourceContribution:
    source: CandidateSource
    weight: float = 1.0

    def __post_init__(self) -> None:
        object.__setattr__(self, "source", _source(self.source, "candidate source"))
        object.__setattr__(self, "weight", _unit_number(self.weight, "source contribution weight"))

    def normalized(self) -> CandidateSourceContribution:
        return CandidateSourceContribution(self.source, clamp_score(self.weight))


@dataclass(frozen=True, slots=True)
class Candidate:
    object_id: ObjectId
    source: CandidateSource
    sources: tuple[CandidateSourceContribution, ...] = ()
    signals: ObjectSignals = field(default_factory=ObjectSignals)

    def __post_init__(self) -> None:
        object_id = _object_id(self.object_id)
        source = _source(self.source, "candidate source")
        if type(self.sources) is not tuple:
            raise ValueError("candidate sources must be a tuple")
        if type(self.signals) is not ObjectSignals:
            raise ValueError("candidate signals must be ObjectSignals")

        normalized_sources: list[CandidateSourceContribution] = []
        seen: set[CandidateSource] = set()
        for contribution in cast(tuple[object, ...], self.sources):
            if not isinstance(contribution, CandidateSourceContribution):
                raise ValueError(
                    "candidate sources must contain CandidateSourceContribution values"
                )
            if contribution.source in seen:
                raise ValueError(f"duplicate candidate source: {contribution.source}")
            seen.add(contribution.source)
            normalized_sources.append(contribution)
        if normalized_sources and source not in seen:
            raise ValueError("candidate sources must include the primary source")

        object.__setattr__(self, "object_id", object_id)
        object.__setattr__(self, "source", source)
        object.__setattr__(self, "sources", tuple(normalized_sources))

    def normalized(self) -> Candidate:
        sources = self.sources or (CandidateSourceContribution(self.source),)
        return Candidate(
            object_id=self.object_id,
            source=self.source,
            sources=tuple(source.normalized() for source in sources),
            signals=self.signals.normalized(),
        )


@dataclass(frozen=True, slots=True)
class LensContribution:
    lens: str
    weight: float
    score: float
    reason: str

    def __post_init__(self) -> None:
        object.__setattr__(self, "lens", _label(self.lens, "lens"))
        object.__setattr__(self, "weight", _unit_number(self.weight, "lens contribution weight"))
        object.__setattr__(self, "score", _unit_number(self.score, "lens contribution score"))
        object.__setattr__(self, "reason", _label(self.reason, "lens contribution reason"))


@dataclass(frozen=True, slots=True)
class RankedCandidate:
    candidate: Candidate
    score: float
    contributions: tuple[LensContribution, ...]

    def __post_init__(self) -> None:
        if not isinstance(cast(object, self.candidate), Candidate):
            raise ValueError("ranked candidate must contain Candidate")
        if type(self.contributions) is not tuple:
            raise ValueError("ranked candidate contributions must be a tuple")
        for contribution in cast(tuple[object, ...], self.contributions):
            if not isinstance(contribution, LensContribution):
                raise ValueError(
                    "ranked candidate contributions must contain LensContribution values"
                )
        object.__setattr__(self, "score", _unit_number(self.score, "ranked score"))


@dataclass(frozen=True, slots=True)
class RankingTrace:
    ranked: tuple[RankedCandidate, ...]

    def __post_init__(self) -> None:
        if type(self.ranked) is not tuple:
            raise ValueError("ranking trace ranked candidates must be a tuple")
        seen: set[ObjectId] = set()
        for ranked in cast(tuple[object, ...], self.ranked):
            if not isinstance(ranked, RankedCandidate):
                raise ValueError("ranking trace must contain RankedCandidate values")
            object_id = ranked.candidate.object_id
            if object_id in seen:
                raise ValueError(f"duplicate ranking trace candidate object_id: {object_id}")
            seen.add(object_id)
