from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Literal, NewType

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


def clamp_score(value: float) -> float:
    if isinstance(value, bool) or not math.isfinite(value):
        return 0.0
    return max(0.0, min(1.0, value))


def nonnegative_signal(value: float) -> float:
    if isinstance(value, bool) or not math.isfinite(value):
        return 0.0
    return max(0.0, value)


@dataclass(frozen=True, slots=True)
class ReputationSignals:
    epistemic_accuracy: float = 0.0
    evidence_quality: float = 0.0
    social_constructiveness: float = 0.0
    creative_contribution: float = 0.0
    moderation: float = 0.0
    domain_expertise: float = 0.0

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

    def normalized(self) -> CandidateSourceContribution:
        return CandidateSourceContribution(self.source, clamp_score(self.weight))


@dataclass(frozen=True, slots=True)
class Candidate:
    object_id: ObjectId
    source: CandidateSource
    sources: tuple[CandidateSourceContribution, ...] = ()
    signals: ObjectSignals = field(default_factory=ObjectSignals)

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


@dataclass(frozen=True, slots=True)
class RankedCandidate:
    candidate: Candidate
    score: float
    contributions: tuple[LensContribution, ...]


@dataclass(frozen=True, slots=True)
class RankingTrace:
    ranked: tuple[RankedCandidate, ...]
