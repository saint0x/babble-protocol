from __future__ import annotations

import math
from dataclasses import dataclass
from enum import StrEnum
from typing import ClassVar

from babel_algorithms.types import clamp_score

MAX_ENGAGEMENT_COUNT = 9007199254740991


def finite_number(value: object, *, minimum: float = -math.inf, maximum: float = math.inf) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError("expected a finite temporal number")
    try:
        number = float(value)
    except OverflowError:
        raise ValueError("temporal number outside its finite range") from None
    if not math.isfinite(number) or not minimum <= number <= maximum:
        raise ValueError("temporal number outside its finite range")
    return number


def _require_type(value: object, expected: type[object]) -> None:
    if not isinstance(value, expected):
        raise ValueError("invalid temporal value type")


def validate_tags(tags: tuple[str, ...]) -> None:
    _require_type(tags, tuple)
    if len(tags) > 32:
        raise ValueError("expected at most 32 temporal tags")
    for tag in tags:
        _require_type(tag, str)
        try:
            if len(tag.encode("utf-8")) > 64:
                raise ValueError("temporal tag exceeds its byte limit")
        except UnicodeError:
            raise ValueError("temporal tags must contain Unicode scalar values") from None


def validate_content_class(content_class: ContentTimeClass) -> None:
    _require_type(content_class, ContentTimeClass)


class ContentTimeClass(StrEnum):
    NEWS = "news"
    DISCUSSION = "discussion"
    ANALYSIS = "analysis"
    TUTORIAL = "tutorial"
    REFERENCE = "reference"


@dataclass(frozen=True, slots=True)
class EngagementWindow:
    total_views: int = 0
    recent_views: int = 0
    total_interactions: int = 0
    recent_interactions: int = 0

    def __post_init__(self) -> None:
        for count in (
            self.total_views, self.recent_views, self.total_interactions, self.recent_interactions
        ):
            if type(count) is not int or not 0 <= count <= MAX_ENGAGEMENT_COUNT:
                raise ValueError("engagement counts must be nonnegative safe integers")
        if (
            self.recent_views > self.total_views
            or self.recent_interactions > self.total_interactions
        ):
            raise ValueError("recent engagement cannot exceed total engagement")


@dataclass(frozen=True, slots=True)
class TemporalInput:
    content_id: str
    published_at: float
    content_class: ContentTimeClass = ContentTimeClass.DISCUSSION
    engagement: EngagementWindow = EngagementWindow()
    quality_score: float = 0.5
    tags: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        _require_type(self.content_id, str)
        if not self.content_id:
            raise ValueError("temporal content ID must be a nonempty string")
        _ = finite_number(self.published_at)
        validate_content_class(self.content_class)
        _require_type(self.engagement, EngagementWindow)
        _ = finite_number(self.quality_score, minimum=0, maximum=1)
        validate_tags(self.tags)


@dataclass(frozen=True, slots=True)
class TemporalScore:
    content_id: str
    age_hours: float
    recency: float
    decay_rate: float
    time_sensitivity: float
    engagement_velocity: float
    survival_score: float

    def __post_init__(self) -> None:
        _require_type(self.content_id, str)
        if not self.content_id:
            raise ValueError("temporal content ID must be a nonempty string")
        _ = finite_number(self.age_hours, minimum=0)
        _ = finite_number(self.decay_rate, minimum=0.01, maximum=0.5)
        for value in (
            self.recency, self.time_sensitivity, self.engagement_velocity, self.survival_score
        ):
            _ = finite_number(value, minimum=0, maximum=1)


class TemporalScorer:
    class_weights: ClassVar[dict[ContentTimeClass, float]] = {
        ContentTimeClass.NEWS: 1.2,
        ContentTimeClass.DISCUSSION: 1.0,
        ContentTimeClass.ANALYSIS: 0.82,
        ContentTimeClass.TUTORIAL: 0.66,
        ContentTimeClass.REFERENCE: 0.45,
    }
    sensitivity: ClassVar[dict[ContentTimeClass, float]] = {
        ContentTimeClass.NEWS: 0.92,
        ContentTimeClass.DISCUSSION: 0.68,
        ContentTimeClass.ANALYSIS: 0.5,
        ContentTimeClass.TUTORIAL: 0.28,
        ContentTimeClass.REFERENCE: 0.12,
    }

    def score(self, item: TemporalInput, *, reference_time: float) -> TemporalScore:
        reference_time = finite_number(reference_time)
        _require_type(item, TemporalInput)
        elapsed = finite_number(reference_time - item.published_at)
        return self.score_at_age(item, age_hours=max(0.0, elapsed / 3600.0))

    def score_at_age(self, item: TemporalInput, *, age_hours: float) -> TemporalScore:
        """Score an age computed before float conversion, preserving timestamp precision."""
        _require_type(item, TemporalInput)
        age_hours = finite_number(age_hours, minimum=0)
        recency = self.recency(age_hours, item.content_class)
        velocity = self.engagement_velocity(item.engagement, age_hours)
        sensitivity = self.time_sensitivity(item.content_class, item.tags)
        decay_rate = self.decay_rate(
            quality_score=item.quality_score,
            engagement_velocity=velocity,
            time_sensitivity=sensitivity,
        )
        survival = clamp_score(recency * math.exp(-decay_rate * age_hours / 24.0) + 0.22 * velocity)
        return TemporalScore(
            content_id=item.content_id,
            age_hours=age_hours,
            recency=recency,
            decay_rate=decay_rate,
            time_sensitivity=sensitivity,
            engagement_velocity=velocity,
            survival_score=survival,
        )

    def recency(self, age_hours: float, content_class: ContentTimeClass) -> float:
        age_hours = finite_number(age_hours, minimum=0)
        validate_content_class(content_class)
        if age_hours <= 2.0:
            base = 1.0
        elif age_hours <= 24.0:
            base = 0.82
        elif age_hours <= 72.0:
            base = 0.62
        elif age_hours <= 168.0:
            base = 0.42
        elif age_hours <= 720.0:
            base = 0.23
        else:
            base = 0.1
        return clamp_score(base * self.class_weights[content_class])

    def decay_rate(
        self,
        *,
        quality_score: float,
        engagement_velocity: float,
        time_sensitivity: float,
    ) -> float:
        quality_score = finite_number(quality_score, minimum=0, maximum=1)
        engagement_velocity = finite_number(engagement_velocity, minimum=0, maximum=1)
        time_sensitivity = finite_number(time_sensitivity, minimum=0, maximum=1)
        raw = 0.1 + 0.16 * time_sensitivity - 0.12 * clamp_score(quality_score) - 0.1 * clamp_score(
            engagement_velocity
        )
        return max(0.01, min(0.5, raw))

    def time_sensitivity(self, content_class: ContentTimeClass, tags: tuple[str, ...]) -> float:
        validate_content_class(content_class)
        validate_tags(tags)
        score = self.sensitivity[content_class]
        normalized_tags = {tag.casefold() for tag in tags}
        if "time-sensitive" in normalized_tags or "breaking" in normalized_tags:
            score += 0.18
        if "evergreen" in normalized_tags or "reference" in normalized_tags:
            score -= 0.18
        return clamp_score(score)

    def engagement_velocity(self, engagement: EngagementWindow, age_hours: float) -> float:
        _require_type(engagement, EngagementWindow)
        age_hours = finite_number(age_hours, minimum=0)
        if age_hours <= 0.0:
            return 0.0
        view_velocity = engagement.recent_views / max(1, engagement.total_views)
        interaction_velocity = engagement.recent_interactions / max(
            1, engagement.total_interactions
        )
        age_factor = math.exp(-age_hours / (24.0 * 7.0))
        return clamp_score((0.62 * view_velocity + 0.38 * interaction_velocity) * age_factor)
