from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Literal, Protocol

JudgmentDefinition = Literal[
    "babel.judgment.spam.v1",
    "babel.judgment.evidence_quality.v1",
    "babel.judgment.relevance.v1",
    "babel.judgment.relationship.v1",
]


@dataclass(frozen=True, slots=True)
class Judgment:
    definition: JudgmentDefinition
    score: float
    confidence: float
    label: str
    reasons: tuple[str, ...]


class JudgmentProvider(Protocol):
    @property
    def provider_version(self) -> str: ...

    def judge(
        self,
        definition: JudgmentDefinition,
        text: str,
        *,
        context: str = "",
    ) -> Judgment: ...


class LocalJudgmentProvider:
    """Deterministic local rules for development, tests, and provider fallback."""

    provider_version = "babel.local.rules.v1"

    def judge(self, definition: JudgmentDefinition, text: str, *, context: str = "") -> Judgment:
        normalized = _normalize(text)
        context_terms = set(_tokens(context))

        if definition == "babel.judgment.spam.v1":
            return _spam(normalized)
        if definition == "babel.judgment.evidence_quality.v1":
            return _evidence_quality(normalized)
        if definition == "babel.judgment.relevance.v1":
            return _relevance(normalized, context_terms)
        if definition == "babel.judgment.relationship.v1":
            return _relationship(normalized)


def _normalize(text: str) -> str:
    return " ".join(text.casefold().split())


def _tokens(text: str) -> tuple[str, ...]:
    return tuple(re.findall(r"[a-z0-9_]+", text.casefold()))


def _spam(text: str) -> Judgment:
    markers = ("buy now", "free money", "guaranteed", "limited time", "click here")
    hits = tuple(marker for marker in markers if marker in text)
    score = min(1.0, 0.15 + 0.22 * len(hits))
    if "http://" in text or "https://" in text:
        score = min(1.0, score + 0.18)
    return Judgment(
        definition="babel.judgment.spam.v1",
        score=score,
        confidence=0.72 if hits else 0.58,
        label="likely_spam" if score >= 0.55 else "unlikely_spam",
        reasons=hits or ("no strong spam markers",),
    )


def _evidence_quality(text: str) -> Judgment:
    markers = (
        "according to",
        "study",
        "dataset",
        "citation",
        "source",
        "replication",
        "measurement",
    )
    hits = tuple(marker for marker in markers if marker in text)
    score = min(1.0, 0.25 + 0.13 * len(hits))
    return Judgment(
        definition="babel.judgment.evidence_quality.v1",
        score=score,
        confidence=0.66 if hits else 0.48,
        label="evidence_rich" if score >= 0.55 else "thin_evidence",
        reasons=hits or ("no evidence markers",),
    )


def _relevance(text: str, context_terms: set[str]) -> Judgment:
    terms = set(_tokens(text))
    overlap = terms & context_terms
    denominator = max(1, len(context_terms))
    score = min(1.0, len(overlap) / denominator)
    return Judgment(
        definition="babel.judgment.relevance.v1",
        score=score,
        confidence=0.7 if context_terms else 0.35,
        label="relevant" if score >= 0.34 else "weak_match",
        reasons=tuple(sorted(overlap)) or ("no shared context terms",),
    )


def _relationship(text: str) -> Judgment:
    contradiction_markers = ("however", "contradicts", "disputes", "fails to replicate")
    support_markers = ("supports", "confirms", "extends", "replicates")
    contradictions = tuple(marker for marker in contradiction_markers if marker in text)
    supports = tuple(marker for marker in support_markers if marker in text)
    if contradictions:
        return Judgment(
            definition="babel.judgment.relationship.v1",
            score=0.82,
            confidence=0.69,
            label="contradicts",
            reasons=contradictions,
        )
    return Judgment(
        definition="babel.judgment.relationship.v1",
        score=0.72 if supports else 0.4,
        confidence=0.64 if supports else 0.42,
        label="supports" if supports else "unknown",
        reasons=supports or ("no relationship markers",),
    )
