"""Public Lens protocol DTOs. No private personalization state is accepted here."""

from __future__ import annotations

import math
import re
from dataclasses import dataclass
from enum import StrEnum
from typing import Literal, cast

from babble_algorithms.ranking_time import parse_timestamp
from babble_algorithms.types import CandidateSource, CandidateSourceContribution, ReputationSignals

_ALLOWED_SOURCES: frozenset[CandidateSource] = frozenset(
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
_MAX_CANDIDATES = 200
_MAX_WEIGHTS = 8
_MAX_SOURCES = 8
_MAX_SOURCE_FLOORS = 8


class BuiltInLens(StrEnum):
    FOLLOWING = "Following"
    FRIENDS = "Friends"
    RESEARCH = "Research"
    INTELLECTUAL_SERENDIPITY = "IntellectualSerendipity"
    CONTRADICTIONS = "Contradictions"
    EMERGING = "Emerging"
    SLOW_INTERNET = "SlowInternet"
    WEIRD = "Weird"

    @property
    def id(self) -> str:
        return f"babble.lens.{self.name.lower()}.v1"


@dataclass(frozen=True, slots=True)
class EvidenceSignals:
    human_support: float
    judgment_support: float
    human_contradiction: float
    judgment_contradiction: float

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "human_support", _nonnegative_signal(self.human_support, "evidence human_support")
        )
        object.__setattr__(
            self,
            "judgment_support",
            _nonnegative_signal(self.judgment_support, "evidence judgment_support"),
        )
        object.__setattr__(
            self,
            "human_contradiction",
            _nonnegative_signal(self.human_contradiction, "evidence human_contradiction"),
        )
        object.__setattr__(
            self,
            "judgment_contradiction",
            _nonnegative_signal(self.judgment_contradiction, "evidence judgment_contradiction"),
        )

    def support_score(self) -> float:
        return min(1.0, (self.human_support + 0.75 * self.judgment_support) / 3.0)

    def contradiction_score(self) -> float:
        return min(1.0, (self.human_contradiction + 0.75 * self.judgment_contradiction) / 3.0)


@dataclass(frozen=True, slots=True)
class Signals:
    social_distance: float
    followed_author: bool
    relevance: float
    novelty: float
    evidence_quality: float
    contradiction: float
    evidence: EvidenceSignals
    reputation: ReputationSignals
    temporal: float
    exploration: float

    def __post_init__(self) -> None:
        if type(self.followed_author) is not bool:
            raise ValueError("followed_author must be a boolean")
        object.__setattr__(
            self, "social_distance", _unit_signal(self.social_distance, "social_distance")
        )
        object.__setattr__(self, "relevance", _unit_signal(self.relevance, "relevance"))
        object.__setattr__(self, "novelty", _unit_signal(self.novelty, "novelty"))
        object.__setattr__(
            self, "evidence_quality", _unit_signal(self.evidence_quality, "evidence_quality")
        )
        object.__setattr__(self, "contradiction", _unit_signal(self.contradiction, "contradiction"))
        object.__setattr__(self, "evidence", _evidence_signals(self.evidence))
        object.__setattr__(self, "reputation", _reputation_signals(self.reputation))
        object.__setattr__(self, "temporal", _unit_signal(self.temporal, "temporal"))
        object.__setattr__(self, "exploration", _unit_signal(self.exploration, "exploration"))


@dataclass(frozen=True, slots=True)
class Candidate:
    object_id: str
    source: CandidateSource
    sources: tuple[CandidateSourceContribution, ...]
    created_at: str
    signals: Signals

    def __post_init__(self) -> None:
        sources = _source_contributions(self.sources, primary=self.source)
        object.__setattr__(self, "object_id", _object_id(self.object_id))
        object.__setattr__(self, "source", _source(self.source, "candidate primary source"))
        object.__setattr__(self, "sources", sources)
        object.__setattr__(self, "created_at", _timestamp(self.created_at))
        object.__setattr__(self, "signals", _signals(self.signals))


@dataclass(frozen=True, slots=True)
class LensWeight:
    lens: BuiltInLens
    weight: float

    def __post_init__(self) -> None:
        if type(self.lens) is not BuiltInLens:
            raise ValueError("ranking lens weights must be typed")
        object.__setattr__(self, "weight", _weight_value(self.weight))


@dataclass(frozen=True, slots=True)
class LensStack:
    id: str
    weights: tuple[LensWeight, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "id", _visible_ascii(self.id, "ranking lens stack ID", 128))
        object.__setattr__(self, "weights", _lens_weights(self.weights))


@dataclass(frozen=True, slots=True)
class SourceFloor:
    source: CandidateSource
    minimum: int

    def __post_init__(self) -> None:
        object.__setattr__(self, "source", _source(self.source, "diversity source floor"))
        object.__setattr__(self, "minimum", _count(self.minimum, "diversity source floor"))


@dataclass(frozen=True, slots=True)
class DiversityPolicy:
    max_source_share: float
    source_floors: tuple[SourceFloor, ...]

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            "max_source_share",
            _unit_signal(self.max_source_share, "diversity max_source_share"),
        )
        object.__setattr__(self, "source_floors", _source_floors(self.source_floors))


@dataclass(frozen=True, slots=True)
class RankingRequest:
    candidates: tuple[Candidate, ...]
    lens: LensStack
    diversity: DiversityPolicy
    limit: int

    def __post_init__(self) -> None:
        object.__setattr__(self, "candidates", _candidates(self.candidates))
        object.__setattr__(self, "lens", _lens_stack(self.lens))
        object.__setattr__(self, "diversity", _diversity_policy(self.diversity))
        object.__setattr__(self, "limit", _count(self.limit, "ranking limit", minimum=1))


@dataclass(frozen=True, slots=True)
class Reason:
    signal: str
    contribution: float

    def __post_init__(self) -> None:
        object.__setattr__(self, "signal", _label(self.signal, "ranking reason signal"))
        object.__setattr__(
            self, "contribution", _finite_number(self.contribution, "ranking reason")
        )


@dataclass(frozen=True, slots=True)
class RankedCandidate:
    candidate: Candidate
    score: float
    reasons: tuple[Reason, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "candidate", _candidate_value(self.candidate))
        object.__setattr__(self, "score", _nonnegative_signal(self.score, "ranked score"))
        object.__setattr__(self, "reasons", _reasons(self.reasons))


@dataclass(frozen=True, slots=True)
class LensContribution:
    lens_id: str
    weight: float
    score: float
    reasons: tuple[Reason, ...]

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "lens_id", _visible_ascii(self.lens_id, "lens contribution ID", 128)
        )
        object.__setattr__(self, "weight", _unit_signal(self.weight, "lens contribution weight"))
        object.__setattr__(
            self, "score", _nonnegative_signal(self.score, "lens contribution score")
        )
        object.__setattr__(self, "reasons", _reasons(self.reasons))


@dataclass(frozen=True, slots=True)
class CandidateTrace:
    rank: int
    object_id: str
    source: CandidateSource
    sources: tuple[CandidateSourceContribution, ...]
    score: float
    lens_contributions: tuple[LensContribution, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "rank", _count(self.rank, "candidate trace rank", minimum=1))
        object.__setattr__(self, "object_id", _object_id(self.object_id))
        object.__setattr__(self, "source", _source(self.source, "candidate trace source"))
        object.__setattr__(
            self, "sources", _source_contributions(self.sources, primary=self.source)
        )
        object.__setattr__(self, "score", _nonnegative_signal(self.score, "candidate trace score"))
        object.__setattr__(self, "lens_contributions", _lens_contributions(self.lens_contributions))


@dataclass(frozen=True, slots=True)
class RankingTrace:
    stack_id: str
    candidates: tuple[CandidateTrace, ...]

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "stack_id", _visible_ascii(self.stack_id, "ranking trace stack ID", 128)
        )
        object.__setattr__(self, "candidates", _candidate_traces(self.candidates))


@dataclass(frozen=True, slots=True)
class DiversifiedCandidateTrace:
    rank: int
    object_id: str
    source: CandidateSource
    lens_score: float
    diversified_score: float
    reasons: tuple[Reason, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "rank", _count(self.rank, "diversity trace rank", minimum=1))
        object.__setattr__(self, "object_id", _object_id(self.object_id))
        object.__setattr__(self, "source", _source(self.source, "diversity trace source"))
        object.__setattr__(self, "lens_score", _nonnegative_signal(self.lens_score, "lens score"))
        object.__setattr__(
            self,
            "diversified_score",
            _unit_signal(self.diversified_score, "diversified score"),
        )
        object.__setattr__(self, "reasons", _reasons(self.reasons))


@dataclass(frozen=True, slots=True)
class DiversityTrace:
    policy: DiversityPolicy
    candidates: tuple[DiversifiedCandidateTrace, ...]
    filtered: tuple[str, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "policy", _diversity_policy(self.policy))
        object.__setattr__(self, "candidates", _diversified_traces(self.candidates))
        object.__setattr__(self, "filtered", _object_ids(self.filtered, "filtered"))


@dataclass(frozen=True, slots=True)
class RankingProvider:
    provider: Literal["babble-python"] = "babble-python"
    model: Literal["lenses-v1"] = "lenses-v1"
    version: Literal["1"] = "1"


RANKING_PROVIDER = RankingProvider()


@dataclass(frozen=True, slots=True)
class RankingResult:
    ranked: tuple[RankedCandidate, ...]
    trace: RankingTrace
    diversity_trace: DiversityTrace
    provider: RankingProvider = RANKING_PROVIDER

    def __post_init__(self) -> None:
        object.__setattr__(self, "ranked", _ranked_candidates(self.ranked))
        object.__setattr__(self, "trace", _ranking_trace(self.trace))
        object.__setattr__(self, "diversity_trace", _diversity_trace(self.diversity_trace))
        object.__setattr__(self, "provider", _ranking_provider(self.provider))


def _source(value: object, label: str) -> CandidateSource:
    if value not in _ALLOWED_SOURCES:
        raise ValueError(f"{label} must be a supported source")
    return value


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


def _finite_number(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise ValueError(f"{label} must be finite")
    result = float(value)
    if not math.isfinite(result):
        raise ValueError(f"{label} must be finite")
    return result


def _weight_value(value: object) -> float:
    if (
        isinstance(value, bool)
        or not isinstance(value, int | float)
        or not math.isfinite(value)
        or value < 0.0
    ):
        raise ValueError("lens weights must be finite and non-negative")
    return float(value)


def _count(value: object, label: str, *, minimum: int = 0, maximum: int = 200) -> int:
    if type(value) is not int or not minimum <= value <= maximum:
        raise ValueError(f"{label} must be a bounded integer")
    return value


def _bounded_tuple(value: object, label: str, maximum: int) -> tuple[object, ...]:
    if type(value) is not tuple:
        raise ValueError(f"{label} must be a bounded tuple")
    items = cast(tuple[object, ...], value)
    if len(items) > maximum:
        raise ValueError(f"{label} must be a bounded tuple")
    return items


def _label(value: object, label: str) -> str:
    if type(value) is not str or not value.strip():
        raise ValueError(f"{label} must be a nonempty string")
    return value.strip()


def _visible_ascii(value: object, label: str, maximum: int) -> str:
    if type(value) is not str or not value or len(value) > maximum:
        raise ValueError(f"{label} must be visible ASCII")
    if any(not 0x21 <= ord(char) <= 0x7E for char in value):
        raise ValueError(f"{label} must be visible ASCII")
    return value


def _object_id(value: object) -> str:
    if type(value) is not str or re.fullmatch(r"obj_[0-9a-f]{64}", value) is None:
        raise ValueError("ranking object_id must be a canonical object id")
    return value


def _timestamp(value: object) -> str:
    if type(value) is not str:
        raise ValueError("ranking timestamp must be a string")
    try:
        canonical, _ = parse_timestamp(value)
    except ValueError:
        raise ValueError("ranking timestamp must be canonicalizable") from None
    return canonical


def _evidence_signals(value: object) -> EvidenceSignals:
    if not isinstance(value, EvidenceSignals):
        raise ValueError("ranking evidence signals must be typed")
    return value


def _reputation_signals(value: object) -> ReputationSignals:
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


def _signals(value: object) -> Signals:
    if not isinstance(value, Signals):
        raise ValueError("ranking signals must be typed")
    return value


def _source_contributions(
    value: object, *, primary: object | None = None
) -> tuple[CandidateSourceContribution, ...]:
    items = _bounded_tuple(value, "candidate source contributions", _MAX_SOURCES)
    if not items:
        raise ValueError("candidate source contributions must be nonempty")
    normalized: list[CandidateSourceContribution] = []
    seen: set[CandidateSource] = set()
    for item in items:
        if type(item) is not CandidateSourceContribution:
            raise ValueError("candidate source contributions must be typed")
        source = _source(item.source, "candidate source")
        if source in seen:
            raise ValueError("candidate source contributions must be unique")
        seen.add(source)
        normalized.append(
            CandidateSourceContribution(
                source, _unit_signal(item.weight, "candidate source weight")
            )
        )
    if primary is not None and _source(primary, "candidate primary source") not in seen:
        raise ValueError("candidate sources must include the primary source")
    return tuple(normalized)


def _candidate_value(value: object) -> Candidate:
    if not isinstance(value, Candidate):
        raise ValueError("ranking candidate must be Candidate")
    return value


def _candidates(value: object) -> tuple[Candidate, ...]:
    items = _bounded_tuple(value, "ranking candidates", _MAX_CANDIDATES)
    normalized: list[Candidate] = []
    seen: set[str] = set()
    for item in items:
        candidate = _candidate_value(item)
        if candidate.object_id in seen:
            raise ValueError("ranking candidates must have unique IDs")
        seen.add(candidate.object_id)
        normalized.append(candidate)
    return tuple(normalized)


def _lens_weights(value: object) -> tuple[LensWeight, ...]:
    items = _bounded_tuple(value, "ranking lens weights", _MAX_WEIGHTS)
    normalized: list[LensWeight] = []
    seen: set[BuiltInLens] = set()
    for item in items:
        if not isinstance(item, LensWeight):
            raise ValueError("ranking lens weights must be typed")
        if item.lens in seen:
            raise ValueError("ranking lens weights must be unique")
        seen.add(item.lens)
        normalized.append(item)
    return tuple(normalized)


def _lens_stack(value: object) -> LensStack:
    if not isinstance(value, LensStack):
        raise ValueError("ranking lens stack must be typed")
    return value


def _source_floors(value: object) -> tuple[SourceFloor, ...]:
    items = _bounded_tuple(value, "diversity source floors", _MAX_SOURCE_FLOORS)
    normalized: list[SourceFloor] = []
    seen: set[CandidateSource] = set()
    for item in items:
        if not isinstance(item, SourceFloor):
            raise ValueError("diversity source floors must be typed")
        if item.source in seen:
            raise ValueError("diversity source floors must be unique")
        seen.add(item.source)
        normalized.append(item)
    return tuple(normalized)


def _diversity_policy(value: object) -> DiversityPolicy:
    if not isinstance(value, DiversityPolicy):
        raise ValueError("ranking diversity policy must be typed")
    return value


def _reasons(value: object) -> tuple[Reason, ...]:
    items = _bounded_tuple(value, "ranking reasons", _MAX_CANDIDATES)
    normalized: list[Reason] = []
    for item in items:
        if not isinstance(item, Reason):
            raise ValueError("ranking reasons must contain Reason values")
        normalized.append(item)
    return tuple(normalized)


def _ranked_candidates(value: object) -> tuple[RankedCandidate, ...]:
    items = _bounded_tuple(value, "ranked candidates", _MAX_CANDIDATES)
    normalized: list[RankedCandidate] = []
    seen: set[str] = set()
    for item in items:
        if not isinstance(item, RankedCandidate):
            raise ValueError("ranked candidates must contain RankedCandidate values")
        if item.candidate.object_id in seen:
            raise ValueError("ranked candidates must have unique IDs")
        seen.add(item.candidate.object_id)
        normalized.append(item)
    return tuple(normalized)


def _lens_contributions(value: object) -> tuple[LensContribution, ...]:
    items = _bounded_tuple(value, "lens contributions", _MAX_WEIGHTS)
    normalized: list[LensContribution] = []
    seen: set[str] = set()
    for item in items:
        if not isinstance(item, LensContribution):
            raise ValueError("lens contributions must contain LensContribution values")
        if item.lens_id in seen:
            raise ValueError("lens contributions must have unique IDs")
        seen.add(item.lens_id)
        normalized.append(item)
    return tuple(normalized)


def _candidate_traces(value: object) -> tuple[CandidateTrace, ...]:
    items = _bounded_tuple(value, "candidate traces", _MAX_CANDIDATES)
    normalized: list[CandidateTrace] = []
    seen: set[str] = set()
    for item in items:
        if not isinstance(item, CandidateTrace):
            raise ValueError("candidate traces must contain CandidateTrace values")
        if item.object_id in seen:
            raise ValueError("candidate traces must have unique IDs")
        seen.add(item.object_id)
        normalized.append(item)
    return tuple(normalized)


def _diversified_traces(value: object) -> tuple[DiversifiedCandidateTrace, ...]:
    items = _bounded_tuple(value, "diversified candidate traces", _MAX_CANDIDATES)
    normalized: list[DiversifiedCandidateTrace] = []
    seen: set[str] = set()
    for item in items:
        if not isinstance(item, DiversifiedCandidateTrace):
            raise ValueError("diversified traces must contain DiversifiedCandidateTrace values")
        if item.object_id in seen:
            raise ValueError("diversified traces must have unique IDs")
        seen.add(item.object_id)
        normalized.append(item)
    return tuple(normalized)


def _ranking_trace(value: object) -> RankingTrace:
    if not isinstance(value, RankingTrace):
        raise ValueError("ranking trace must be RankingTrace")
    return value


def _diversity_trace(value: object) -> DiversityTrace:
    if not isinstance(value, DiversityTrace):
        raise ValueError("diversity trace must be DiversityTrace")
    return value


def _ranking_provider(value: object) -> RankingProvider:
    if not isinstance(value, RankingProvider):
        raise ValueError("ranking provider must be RankingProvider")
    return value


def _object_ids(value: object, label: str) -> tuple[str, ...]:
    items = _bounded_tuple(value, label, _MAX_CANDIDATES)
    normalized: list[str] = []
    seen: set[str] = set()
    for item in items:
        object_id = _object_id(item)
        if object_id in seen:
            raise ValueError(f"{label} must have unique IDs")
        seen.add(object_id)
        normalized.append(object_id)
    return tuple(normalized)
