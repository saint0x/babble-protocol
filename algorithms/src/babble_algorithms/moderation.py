from __future__ import annotations

import math
import re
from dataclasses import dataclass
from typing import ClassVar, Literal, cast

from babble_algorithms.content import ContentAnalyzer
from babble_algorithms.text import sentences, tokens
from babble_algorithms.types import clamp_score

ModerationAction = Literal["allow", "warn", "limit", "flag", "remove"]


@dataclass(frozen=True, slots=True)
class ModerationContext:
    repeated_messages: int = 0
    account_age_days: float = 30.0
    reports: int = 0
    similar_recent_posts: int = 0
    external_links: int = 0

    def __post_init__(self) -> None:
        for value in (
            self.repeated_messages,
            self.reports,
            self.similar_recent_posts,
            self.external_links,
        ):
            if type(value) is not int or not 0 <= value <= 9007199254740991:
                raise ValueError("moderation counts must be nonnegative safe integers")
        if (
            type(self.account_age_days) not in (int, float)
            or not 0 <= self.account_age_days <= 9007199254740991
            or not math.isfinite(self.account_age_days)
        ):
            raise ValueError("account age must be finite and nonnegative")


@dataclass(frozen=True, slots=True)
class ModerationScores:
    spam: float
    quality: float
    sentiment: float
    safety: float
    coordination: float


@dataclass(frozen=True, slots=True)
class ModerationResult:
    content_id: str
    action: ModerationAction
    flags: tuple[str, ...]
    scores: ModerationScores
    reasons: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class ModerationPolicy:
    spam_limit: float = 0.7
    quality_warn: float = 0.32
    safety_remove: float = 0.82
    coordination_limit: float = 0.65

    def __post_init__(self) -> None:
        for value in (
            self.spam_limit,
            self.quality_warn,
            self.safety_remove,
            self.coordination_limit,
        ):
            if type(value) not in (int, float) or not 0 <= value <= 1 or not math.isfinite(value):
                raise ValueError("moderation thresholds must be finite numbers in [0, 1]")


class CommunityModerator:
    promotional_terms: ClassVar[tuple[str, ...]] = (
        "buy now",
        "click here",
        "free money",
        "limited time",
        "special offer",
        "sign up now",
        "guaranteed",
        "money back",
        "best price",
    )
    urgency_terms: ClassVar[tuple[str, ...]] = (
        "urgent",
        "hurry",
        "last chance",
        "expires",
        "today only",
        "act now",
        "don't wait",
    )
    harassment_terms: ClassVar[frozenset[str]] = frozenset(
        {
            "idiot",
            "stupid",
            "worthless",
            "trash",
            "kill",
            "die",
            "attack",
            "harass",
        }
    )
    misinformation_markers: ClassVar[tuple[str, ...]] = (
        "they do not want you to know",
        "secret cure",
        "proven hoax",
        "mainstream media hides",
        "100% fabricated",
        "no evidence needed",
    )

    def __init__(self, policy: ModerationPolicy | None = None) -> None:
        self.policy: ModerationPolicy = policy or ModerationPolicy()
        self.content: ContentAnalyzer = ContentAnalyzer()

    def analyze(
        self,
        content_id: str,
        text: str,
        *,
        context: ModerationContext | None = None,
    ) -> ModerationResult:
        moderation_context = context or ModerationContext()
        spam_score, spam_reasons = self._spam_score(text, moderation_context)
        quality_score, quality_reasons = self._quality_score(text)
        sentiment_score = self.content.analyze(content_id, text).sentiment_score
        safety_score, safety_reasons = self._safety_score(text)
        coordination_score, coordination_reasons = self._coordination_score(moderation_context)

        flags: list[str] = []
        reasons: list[str] = []

        if safety_score >= self.policy.safety_remove:
            flags.append("safety")
            reasons.extend(safety_reasons)
        if spam_score >= self.policy.spam_limit:
            flags.append("spam")
            reasons.extend(spam_reasons)
        if coordination_score >= self.policy.coordination_limit:
            flags.append("coordinated_behavior")
            reasons.extend(coordination_reasons)
        if quality_score <= self.policy.quality_warn:
            flags.append("low_quality")
            reasons.extend(quality_reasons)
        if any(marker in text.casefold() for marker in self.misinformation_markers):
            flags.append("misinformation_pattern")
            reasons.append("lexical misinformation framing marker; not a factual assessment")

        action = self._action(flags, spam_score, quality_score, safety_score, coordination_score)
        return ModerationResult(
            content_id=content_id,
            action=action,
            flags=tuple(dict.fromkeys(flags)),
            scores=ModerationScores(
                spam=spam_score,
                quality=quality_score,
                sentiment=sentiment_score,
                safety=safety_score,
                coordination=coordination_score,
            ),
            reasons=tuple(dict.fromkeys(reasons)) or ("no moderation thresholds exceeded",),
        )

    def _spam_score(self, text: str, context: ModerationContext) -> tuple[float, tuple[str, ...]]:
        normalized = text.casefold()
        words = tokens(text, remove_stop_words=False)
        reasons: list[str] = []
        score = 0.0

        promotional_hits = sum(1 for term in self.promotional_terms if term in normalized)
        urgency_hits = sum(1 for term in self.urgency_terms if term in normalized)
        link_hits = len(re.findall(r"https?://|www\.", normalized)) + context.external_links
        # Tokenize candidates once: restarting an email regex at every word boundary
        # is quadratic for long dotted text without an @ sign.
        email_candidates = cast(tuple[str, ...], tuple(re.findall(r"[\w.%+@-]+", normalized)))
        email_hits = sum(
            re.fullmatch(r"[\w.%+-]+@[\w.-]+\.[a-z]{2,}", candidate.strip(".")) is not None
            for candidate in email_candidates
        )

        if promotional_hits:
            score += min(0.42, promotional_hits * 0.12)
            reasons.append("promotional language")
        if urgency_hits:
            score += min(0.24, urgency_hits * 0.08)
            reasons.append("artificial urgency")
        if link_hits:
            score += min(0.22, link_hits * 0.08)
            reasons.append("external-link pressure")
        if email_hits:
            score += min(0.12, email_hits * 0.06)
            reasons.append("contact harvesting pattern")
        if words:
            repetition = 1.0 - (len(set(words)) / len(words))
            score += min(0.18, repetition * 0.35)
            if repetition > 0.35:
                reasons.append("repetitive phrasing")
        caps_ratio = sum(1 for char in text if char.isupper()) / max(1, len(text))
        if caps_ratio > 0.22:
            score += min(0.16, caps_ratio * 0.45)
            reasons.append("excessive capitalization")
        if context.repeated_messages > 0:
            score += min(0.18, context.repeated_messages * 0.06)
            reasons.append("repeated-message context")
        punctuation_bursts = len(re.findall(r"[!?]{2,}", text))
        if punctuation_bursts:
            score += min(0.08, punctuation_bursts * 0.08)
            reasons.append("punctuation burst")

        return clamp_score(score), tuple(reasons)

    def _quality_score(self, text: str) -> tuple[float, tuple[str, ...]]:
        words = tokens(text)
        sentence_values = sentences(text)
        analysis = self.content.analyze("quality", text)
        reasons: list[str] = []

        length_score = min(1.0, len(words) / 80.0)
        if len(words) < 8:
            reasons.append("very short text")
        capitalization_score = 1.0
        if sentence_values:
            capitalization_score = sum(1 for item in sentence_values if item[:1].isupper()) / len(
                sentence_values
            )
            if capitalization_score < 0.6:
                reasons.append("weak sentence formatting")
        evidence_score = analysis.evidence.strength_score
        complexity_score = analysis.complexity_score
        return (
            clamp_score(
                0.25 * length_score
                + 0.2 * capitalization_score
                + 0.25 * complexity_score
                + 0.2 * evidence_score
                + 0.1 * (1.0 - max(0.0, -analysis.sentiment_score))
            ),
            tuple(reasons),
        )

    def _safety_score(self, text: str) -> tuple[float, tuple[str, ...]]:
        word_set = set(tokens(text, remove_stop_words=False))
        hits = tuple(sorted(word_set & self.harassment_terms))
        threat_patterns = (
            "you should die",
            "go die",
            "i will hurt",
            "we should attack",
            "target them",
        )
        pattern_hits = tuple(pattern for pattern in threat_patterns if pattern in text.casefold())
        score = clamp_score(0.16 * len(hits) + 0.34 * len(pattern_hits))
        reasons = tuple(f"abusive term: {term}" for term in hits) + tuple(
            f"threat pattern: {pattern}" for pattern in pattern_hits
        )
        return score, reasons

    def _coordination_score(self, context: ModerationContext) -> tuple[float, tuple[str, ...]]:
        score = clamp_score(
            0.18 * context.reports
            + 0.12 * context.similar_recent_posts
            + (0.18 if context.account_age_days < 2.0 else 0.0)
        )
        reasons: list[str] = []
        if context.reports:
            reasons.append("community reports")
        if context.similar_recent_posts:
            reasons.append("similar recent posts")
        if context.account_age_days < 2.0:
            reasons.append("new-account burst")
        return score, tuple(reasons)

    def _action(
        self,
        flags: list[str],
        spam_score: float,
        quality_score: float,
        safety_score: float,
        coordination_score: float,
    ) -> ModerationAction:
        if safety_score >= self.policy.safety_remove or ("spam" in flags and spam_score >= 0.88):
            return "remove"
        if (
            "misinformation_pattern" in flags
            or coordination_score >= self.policy.coordination_limit
        ):
            return "flag"
        if spam_score >= self.policy.spam_limit:
            return "limit"
        if quality_score <= self.policy.quality_warn:
            return "warn"
        return "allow"
