"""Public Lens protocol DTOs. No private personalization state is accepted here."""

from dataclasses import dataclass
from enum import StrEnum
from typing import Literal

from babble_algorithms.types import CandidateSource, CandidateSourceContribution, ReputationSignals


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


@dataclass(frozen=True, slots=True)
class Candidate:
    object_id: str
    source: CandidateSource
    sources: tuple[CandidateSourceContribution, ...]
    created_at: str
    signals: Signals


@dataclass(frozen=True, slots=True)
class LensWeight:
    lens: BuiltInLens
    weight: float


@dataclass(frozen=True, slots=True)
class LensStack:
    id: str
    weights: tuple[LensWeight, ...]


@dataclass(frozen=True, slots=True)
class SourceFloor:
    source: CandidateSource
    minimum: int


@dataclass(frozen=True, slots=True)
class DiversityPolicy:
    max_source_share: float
    source_floors: tuple[SourceFloor, ...]


@dataclass(frozen=True, slots=True)
class RankingRequest:
    candidates: tuple[Candidate, ...]
    lens: LensStack
    diversity: DiversityPolicy
    limit: int


@dataclass(frozen=True, slots=True)
class Reason:
    signal: str
    contribution: float


@dataclass(frozen=True, slots=True)
class RankedCandidate:
    candidate: Candidate
    score: float
    reasons: tuple[Reason, ...]


@dataclass(frozen=True, slots=True)
class LensContribution:
    lens_id: str
    weight: float
    score: float
    reasons: tuple[Reason, ...]


@dataclass(frozen=True, slots=True)
class CandidateTrace:
    rank: int
    object_id: str
    source: CandidateSource
    sources: tuple[CandidateSourceContribution, ...]
    score: float
    lens_contributions: tuple[LensContribution, ...]


@dataclass(frozen=True, slots=True)
class RankingTrace:
    stack_id: str
    candidates: tuple[CandidateTrace, ...]


@dataclass(frozen=True, slots=True)
class DiversifiedCandidateTrace:
    rank: int
    object_id: str
    source: CandidateSource
    lens_score: float
    diversified_score: float
    reasons: tuple[Reason, ...]


@dataclass(frozen=True, slots=True)
class DiversityTrace:
    policy: DiversityPolicy
    candidates: tuple[DiversifiedCandidateTrace, ...]
    filtered: tuple[str, ...]


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
