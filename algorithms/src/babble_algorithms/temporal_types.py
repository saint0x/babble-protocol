"""Public temporal worker contracts; engagement contains supplied aggregate counts only."""

import re
from dataclasses import dataclass
from typing import Literal, cast

from babble_algorithms.ranking_time import timestamp_nanos
from babble_algorithms.temporal import (
    ContentTimeClass,
    EngagementWindow,
    finite_number,
    validate_content_class,
    validate_tags,
)

_MAX_TEMPORAL_ITEMS = 200
_OBJECT_ID = re.compile(r"obj_[0-9a-f]{64}\Z")


def _timestamp(value: object, label: str) -> str:
    if type(value) is not str:
        raise ValueError(f"{label} must be an RFC3339 timestamp string")
    _ = timestamp_nanos(value)
    return value


def _object_id(value: object) -> str:
    if type(value) is not str or _OBJECT_ID.fullmatch(value) is None:
        raise ValueError("temporal object_id must be obj_ followed by 64 lowercase hex digits")
    return value


@dataclass(frozen=True, slots=True)
class TemporalItem:
    object_id: str
    published_at: str
    content_class: ContentTimeClass
    quality_score: float
    tags: tuple[str, ...]
    engagement: EngagementWindow

    def __post_init__(self) -> None:
        _ = _object_id(self.object_id)
        _ = _timestamp(self.published_at, "published_at")
        validate_content_class(self.content_class)
        _ = finite_number(self.quality_score, minimum=0, maximum=1)
        validate_tags(self.tags)
        if type(self.engagement) is not EngagementWindow:
            raise ValueError("temporal engagement must be an EngagementWindow")


@dataclass(frozen=True, slots=True)
class TemporalRequest:
    reference_time: str
    items: tuple[TemporalItem, ...]

    def __post_init__(self) -> None:
        _ = _timestamp(self.reference_time, "reference_time")
        if type(self.items) is not tuple or len(self.items) > _MAX_TEMPORAL_ITEMS:
            raise ValueError("temporal items must be a bounded tuple")
        seen: set[str] = set()
        for item in self.items:
            if type(item) is not TemporalItem:
                raise ValueError("temporal items must be TemporalItem values")
            if item.object_id in seen:
                raise ValueError("temporal items must have unique object IDs")
            seen.add(item.object_id)


@dataclass(frozen=True, slots=True)
class TemporalProvider:
    provider: Literal["babble-python"] = "babble-python"
    model: Literal["temporal-v1"] = "temporal-v1"
    version: Literal["1"] = "1"

    def __post_init__(self) -> None:
        if self.provider != "babble-python" or self.model != "temporal-v1" or self.version != "1":
            raise ValueError("temporal provider identity is invalid")


TEMPORAL_PROVIDER = TemporalProvider()


@dataclass(frozen=True, slots=True)
class TemporalOutput:
    object_id: str
    age_hours: float
    recency: float
    decay_rate: float
    time_sensitivity: float
    engagement_velocity: float
    survival_score: float

    def __post_init__(self) -> None:
        object.__setattr__(self, "object_id", _object_id(self.object_id))
        object.__setattr__(self, "age_hours", finite_number(self.age_hours, minimum=0))
        object.__setattr__(self, "recency", finite_number(self.recency, minimum=0, maximum=1))
        object.__setattr__(
            self, "decay_rate", finite_number(self.decay_rate, minimum=0.01, maximum=0.5)
        )
        object.__setattr__(
            self,
            "time_sensitivity",
            finite_number(self.time_sensitivity, minimum=0, maximum=1),
        )
        object.__setattr__(
            self,
            "engagement_velocity",
            finite_number(self.engagement_velocity, minimum=0, maximum=1),
        )
        object.__setattr__(
            self, "survival_score", finite_number(self.survival_score, minimum=0, maximum=1)
        )


@dataclass(frozen=True, slots=True)
class TemporalResult:
    provider: TemporalProvider
    reference_time: str
    scores: tuple[TemporalOutput, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "provider", _provider(self.provider))
        object.__setattr__(
            self, "reference_time", _timestamp(self.reference_time, "reference_time")
        )
        object.__setattr__(self, "scores", _scores(self.scores))


def _provider(value: object) -> TemporalProvider:
    if not isinstance(value, TemporalProvider):
        raise ValueError("temporal provider must be TemporalProvider")
    return value


def _scores(value: object) -> tuple[TemporalOutput, ...]:
    if type(value) is not tuple:
        raise ValueError("temporal scores must be a bounded tuple")
    values = cast(tuple[object, ...], value)
    if len(values) > _MAX_TEMPORAL_ITEMS:
        raise ValueError("temporal scores must be a bounded tuple")
    scores: list[TemporalOutput] = []
    seen: set[str] = set()
    for score in values:
        if not isinstance(score, TemporalOutput):
            raise ValueError("temporal scores must be TemporalOutput values")
        if score.object_id in seen:
            raise ValueError("temporal scores must have unique object IDs")
        seen.add(score.object_id)
        scores.append(score)
    return tuple(scores)
