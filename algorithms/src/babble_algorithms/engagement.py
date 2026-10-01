from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass
from typing import Literal, cast

from babble_algorithms.types import clamp_score

InteractionType = Literal["view", "expand", "react", "reply", "share", "save", "surface_open"]
INTERACTION_TYPES = ("view", "expand", "react", "reply", "share", "save", "surface_open")
MAX_SAFE_INTEGER = 9007199254740991


def _finite_number(
    value: object,
    name: str,
    *,
    minimum: float = -math.inf,
    maximum: float = math.inf,
) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{name} must be a finite real number")
    try:
        number = float(value)
    except OverflowError as error:
        raise ValueError(f"{name} must be a finite real number") from error
    if not math.isfinite(number) or not minimum <= number <= maximum:
        raise ValueError(f"{name} must be a finite real number")
    return number


def _nonempty_string(value: object, name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{name} must be a nonempty string")
    try:
        _ = value.encode("utf-8", errors="strict")
    except UnicodeError:
        raise ValueError(f"{name} must contain Unicode scalar values") from None
    return value


def _count(value: object, name: str, *, minimum: int = 0) -> int:
    if type(value) is not int or not minimum <= value <= MAX_SAFE_INTEGER:
        raise ValueError(f"{name} must be a bounded integer")
    return value


def _hour_tuple(value: object) -> tuple[int, ...]:
    if type(value) is not tuple:
        raise ValueError("peak_hours must be a tuple")
    hours = cast(tuple[object, ...], value)
    result: list[int] = []
    seen: set[int] = set()
    for hour in hours:
        if type(hour) is not int or not 0 <= hour <= 23:
            raise ValueError("peak_hours entries must be hours")
        if hour in seen:
            raise ValueError("peak_hours must be unique")
        seen.add(hour)
        result.append(hour)
    if tuple(result) != tuple(sorted(result)):
        raise ValueError("peak_hours must be sorted")
    return tuple(result)


@dataclass(frozen=True, slots=True)
class EngagementEvent:
    user_id: str
    content_id: str
    timestamp: float
    session_duration_seconds: float
    scroll_depth: float
    interaction: InteractionType = "view"

    def __post_init__(self) -> None:
        object.__setattr__(self, "user_id", _nonempty_string(self.user_id, "engagement user_id"))
        object.__setattr__(
            self, "content_id", _nonempty_string(self.content_id, "engagement content_id")
        )
        object.__setattr__(
            self, "timestamp", _finite_number(self.timestamp, "engagement timestamp")
        )
        object.__setattr__(
            self,
            "session_duration_seconds",
            _finite_number(
                self.session_duration_seconds,
                "engagement session_duration_seconds",
                minimum=0,
                maximum=MAX_SAFE_INTEGER,
            ),
        )
        object.__setattr__(
            self,
            "scroll_depth",
            _finite_number(self.scroll_depth, "engagement scroll_depth", minimum=0, maximum=1),
        )
        if self.interaction not in INTERACTION_TYPES:
            raise ValueError("engagement interaction must be a supported literal")


@dataclass(frozen=True, slots=True)
class ContentPerformance:
    content_id: str
    sessions: int
    avg_session_duration_seconds: float
    avg_scroll_depth: float
    interaction_rate: float
    engagement_score: float

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "content_id", _nonempty_string(self.content_id, "performance content_id")
        )
        object.__setattr__(
            self, "sessions", _count(self.sessions, "performance sessions", minimum=1)
        )
        object.__setattr__(
            self,
            "avg_session_duration_seconds",
            _finite_number(
                self.avg_session_duration_seconds,
                "performance avg_session_duration_seconds",
                minimum=0,
                maximum=MAX_SAFE_INTEGER,
            ),
        )
        object.__setattr__(
            self,
            "avg_scroll_depth",
            _finite_number(
                self.avg_scroll_depth, "performance avg_scroll_depth", minimum=0, maximum=1
            ),
        )
        object.__setattr__(
            self,
            "interaction_rate",
            _finite_number(
                self.interaction_rate, "performance interaction_rate", minimum=0, maximum=1
            ),
        )
        object.__setattr__(
            self,
            "engagement_score",
            _finite_number(
                self.engagement_score, "performance engagement_score", minimum=0, maximum=1
            ),
        )


@dataclass(frozen=True, slots=True)
class EngagementSummary:
    total_sessions: int
    avg_session_duration_seconds: float
    avg_scroll_depth: float
    peak_hours: tuple[int, ...]
    trend: dict[str, float]
    user_segments: dict[str, int]
    content_performance: dict[str, ContentPerformance]

    def __post_init__(self) -> None:
        object.__setattr__(self, "total_sessions", _count(self.total_sessions, "total_sessions"))
        object.__setattr__(
            self,
            "avg_session_duration_seconds",
            _finite_number(
                self.avg_session_duration_seconds,
                "avg_session_duration_seconds",
                minimum=0,
                maximum=MAX_SAFE_INTEGER,
            ),
        )
        object.__setattr__(
            self,
            "avg_scroll_depth",
            _finite_number(self.avg_scroll_depth, "avg_scroll_depth", minimum=0, maximum=1),
        )
        object.__setattr__(self, "peak_hours", _hour_tuple(self.peak_hours))
        if type(self.trend) is not dict:
            raise ValueError("engagement trend must be a dict")
        trend: dict[str, float] = {}
        for period, score in cast(dict[object, object], self.trend).items():
            if not isinstance(period, str) or not period.strip():
                raise ValueError("engagement trend periods must be strings")
            trend[period] = _finite_number(score, "engagement trend score", minimum=0, maximum=1)
        object.__setattr__(self, "trend", trend)
        if type(self.user_segments) is not dict:
            raise ValueError("engagement user_segments must be a dict")
        segments: dict[str, int] = {}
        for segment, count in cast(dict[object, object], self.user_segments).items():
            if not isinstance(segment, str) or not segment.strip():
                raise ValueError("engagement user segment names must be strings")
            segments[segment] = _count(count, "engagement user segment count")
        object.__setattr__(self, "user_segments", segments)
        if type(self.content_performance) is not dict:
            raise ValueError("content_performance must be a dict")
        performance: dict[str, ContentPerformance] = {}
        for content_id, value in cast(dict[object, object], self.content_performance).items():
            key = _nonempty_string(content_id, "content_performance key")
            if type(value) is not ContentPerformance:
                raise ValueError("content_performance values must be ContentPerformance")
            if value.content_id != key:
                raise ValueError("content_performance keys must match content IDs")
            performance[key] = value
        object.__setattr__(self, "content_performance", performance)


@dataclass(frozen=True, slots=True)
class _ScoredEvent:
    event: EngagementEvent
    score: float


class EngagementAnalyzer:
    def summarize(
        self,
        events: tuple[EngagementEvent, ...],
        *,
        reference_time: float,
        window_seconds: float,
    ) -> EngagementSummary:
        if type(events) is not tuple:
            raise ValueError("engagement events must be a tuple")
        typed_events: list[EngagementEvent] = []
        for event in events:
            if type(event) is not EngagementEvent:
                raise ValueError("engagement events must contain EngagementEvent values")
            typed_events.append(event)
        reference_time = _finite_number(reference_time, "reference_time")
        window_seconds = _finite_number(window_seconds, "window_seconds", minimum=0)
        if window_seconds <= 0.0:
            raise ValueError("window_seconds must be positive")
        start = reference_time - window_seconds
        if not math.isfinite(start):
            raise ValueError("engagement window start must be finite")
        scoped = tuple(
            _ScoredEvent(event, _event_score(event))
            for event in typed_events
            if start <= event.timestamp <= reference_time
        )
        if not scoped:
            return EngagementSummary(0, 0.0, 0.0, (), {}, {}, {})

        return EngagementSummary(
            total_sessions=len(scoped),
            avg_session_duration_seconds=sum(item.event.session_duration_seconds for item in scoped)
            / len(scoped),
            avg_scroll_depth=sum(item.event.scroll_depth for item in scoped) / len(scoped),
            peak_hours=self._peak_hours(scoped),
            trend=self._trend(scoped, start, window_seconds),
            user_segments=self._segments(scoped),
            content_performance=self._content_performance(scoped),
        )

    def _peak_hours(self, events: tuple[_ScoredEvent, ...]) -> tuple[int, ...]:
        counts: dict[int, int] = defaultdict(int)
        for item in events:
            hour = int((item.event.timestamp % 86_400) // 3_600)
            counts[hour] += 1
        average = sum(counts.values()) / max(1, len(counts))
        return tuple(sorted(hour for hour, count in counts.items() if count > average))

    def _trend(
        self,
        events: tuple[_ScoredEvent, ...],
        start: float,
        window_seconds: float,
    ) -> dict[str, float]:
        period = window_seconds / 6.0
        buckets: dict[str, list[float]] = {f"period_{index + 1}": [] for index in range(6)}
        for item in events:
            index = min(5, max(0, int((item.event.timestamp - start) / period)))
            buckets[f"period_{index + 1}"].append(item.score)
        return {
            name: (sum(values) / len(values) if values else 0.0) for name, values in buckets.items()
        }

    def _segments(self, events: tuple[_ScoredEvent, ...]) -> dict[str, int]:
        by_user: dict[str, list[float]] = defaultdict(list)
        for item in events:
            by_user[item.event.user_id].append(item.score)

        segments = {"highly_engaged": 0, "moderately_engaged": 0, "low_engagement": 0}
        for scores in by_user.values():
            average = sum(scores) / len(scores)
            if average >= 0.8:
                segments["highly_engaged"] += 1
            elif average >= 0.45:
                segments["moderately_engaged"] += 1
            else:
                segments["low_engagement"] += 1
        return segments

    def _content_performance(
        self, events: tuple[_ScoredEvent, ...]
    ) -> dict[str, ContentPerformance]:
        by_content: dict[str, list[_ScoredEvent]] = defaultdict(list)
        for item in events:
            by_content[item.event.content_id].append(item)

        result: dict[str, ContentPerformance] = {}
        for content_id, values in by_content.items():
            sessions = len(values)
            interaction_count = sum(1 for item in values if item.event.interaction != "view")
            result[content_id] = ContentPerformance(
                content_id=content_id,
                sessions=sessions,
                avg_session_duration_seconds=sum(
                    item.event.session_duration_seconds for item in values
                )
                / sessions,
                avg_scroll_depth=sum(item.event.scroll_depth for item in values) / sessions,
                interaction_rate=interaction_count / sessions,
                engagement_score=sum(item.score for item in values) / sessions,
            )
        return result


def _event_score(event: EngagementEvent) -> float:
    interaction_bonus = {
        "view": 0.0,
        "expand": 0.08,
        "react": 0.12,
        "reply": 0.18,
        "share": 0.2,
        "save": 0.16,
        "surface_open": 0.22,
    }[event.interaction]
    duration_score = min(1.0, event.session_duration_seconds / 300.0)
    return clamp_score(0.48 * event.scroll_depth + 0.38 * duration_score + interaction_bonus)
