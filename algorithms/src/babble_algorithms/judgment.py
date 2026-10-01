from __future__ import annotations

import math
import re
from dataclasses import dataclass
from typing import ClassVar, Literal, Protocol, cast

JudgmentDefinition = Literal[
    "babble.judgment.spam.v1",
    "babble.judgment.evidence_quality.v1",
    "babble.judgment.relevance.v1",
    "babble.judgment.relationship.v1",
]

SUPPORTED_DEFINITIONS: frozenset[JudgmentDefinition] = frozenset(
    {
        "babble.judgment.spam.v1",
        "babble.judgment.evidence_quality.v1",
        "babble.judgment.relevance.v1",
        "babble.judgment.relationship.v1",
    }
)


@dataclass(frozen=True, slots=True)
class Judgment:
    definition: JudgmentDefinition
    score: float
    confidence: float
    label: str
    reasons: tuple[str, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "definition", _definition(self.definition))
        object.__setattr__(self, "score", _unit_score(self.score, "judgment score"))
        object.__setattr__(self, "confidence", _unit_score(self.confidence, "judgment confidence"))
        object.__setattr__(self, "label", _non_empty_text(self.label, "judgment label"))
        object.__setattr__(self, "reasons", _reasons(self.reasons))


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

    provider_version: ClassVar[str] = "babble.local.rules.v1"

    def judge(self, definition: JudgmentDefinition, text: str, *, context: str = "") -> Judgment:
        definition = _definition(definition)
        normalized = _normalize(_text(text, "text"))
        context_terms = set(_tokens(_text(context, "context")))

        if definition == "babble.judgment.spam.v1":
            return _spam(normalized)
        if definition == "babble.judgment.evidence_quality.v1":
            return _evidence_quality(normalized)
        if definition == "babble.judgment.relevance.v1":
            return _relevance(normalized, context_terms)
        return _relationship(normalized)


def _definition(value: object) -> JudgmentDefinition:
    if not isinstance(value, str) or value not in SUPPORTED_DEFINITIONS:
        raise ValueError("unsupported local Judgment definition")
    return value


def _text(value: object, label: str) -> str:
    if not isinstance(value, str):
        raise ValueError(f"{label} must be a string")
    return value


def _unit_score(value: object, label: str) -> float:
    if not isinstance(value, int | float) or isinstance(value, bool):
        raise ValueError(f"{label} must be a finite number between 0 and 1")
    numeric = float(value)
    if not math.isfinite(numeric) or not 0.0 <= numeric <= 1.0:
        raise ValueError(f"{label} must be a finite number between 0 and 1")
    return numeric


def _non_empty_text(value: object, label: str) -> str:
    text = _text(value, label)
    normalized = " ".join(text.split())
    if not normalized:
        raise ValueError(f"{label} must be non-empty")
    return normalized


def _reasons(value: object) -> tuple[str, ...]:
    if type(value) is not tuple:
        raise ValueError("judgment reasons must be a tuple")
    normalized = tuple(
        _non_empty_text(reason, "judgment reason") for reason in cast(tuple[object, ...], value)
    )
    if not normalized:
        raise ValueError("judgment reasons must be non-empty")
    return normalized


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
        definition="babble.judgment.spam.v1",
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
        definition="babble.judgment.evidence_quality.v1",
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
        definition="babble.judgment.relevance.v1",
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
            definition="babble.judgment.relationship.v1",
            score=0.82,
            confidence=0.69,
            label="contradicts",
            reasons=contradictions,
        )
    return Judgment(
        definition="babble.judgment.relationship.v1",
        score=0.72 if supports else 0.4,
        confidence=0.64 if supports else 0.42,
        label="supports" if supports else "unknown",
        reasons=supports or ("no relationship markers",),
    )
