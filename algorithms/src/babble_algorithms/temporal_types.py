"""Public temporal worker contracts; engagement contains supplied aggregate counts only."""

from dataclasses import dataclass
from typing import Literal

from babble_algorithms.temporal import ContentTimeClass, EngagementWindow


@dataclass(frozen=True, slots=True)
class TemporalItem:
    object_id: str
    published_at: str
    content_class: ContentTimeClass
    quality_score: float
    tags: tuple[str, ...]
    engagement: EngagementWindow


@dataclass(frozen=True, slots=True)
class TemporalRequest:
    reference_time: str
    items: tuple[TemporalItem, ...]


@dataclass(frozen=True, slots=True)
class TemporalProvider:
    provider: Literal["babble-python"] = "babble-python"
    model: Literal["temporal-v1"] = "temporal-v1"
    version: Literal["1"] = "1"


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


@dataclass(frozen=True, slots=True)
class TemporalResult:
    provider: TemporalProvider
    reference_time: str
    scores: tuple[TemporalOutput, ...]
