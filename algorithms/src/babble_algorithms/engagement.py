from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass
from typing import Literal, get_args

from babble_algorithms.types import clamp_score

InteractionType = Literal["view", "expand", "react", "reply", "share", "save", "surface_open"]
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


def _nonempty_string(value: object, name: str) -> None:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{name} must be a nonempty string")
    try:
        _ = value.encode("utf-8", errors="strict")
    except UnicodeError:
        raise ValueError(f"{name} must contain Unicode scalar values") from None


@dataclass(frozen=True, slots=True)
class EngagementEvent:
    user_id: str
    content_id: str
    timestamp: float
    session_duration_seconds: float
    scroll_depth: float
    interaction: InteractionType = "view"

    def __post_init__(self) -> None:
        _nonempty_string(self.user_id, "engagement user_id")
        _nonempty_string(self.content_id, "engagement content_id")
        _ = _finite_number(self.timestamp, "engagement timestamp")
        _ = _finite_number(
            self.session_duration_seconds,
            "engagement session_duration_seconds",
            minimum=0,
            maximum=MAX_SAFE_INTEGER,
        )
        _ = _finite_number(self.scroll_depth, "engagement scroll_depth", minimum=0, maximum=1)
        if self.interaction not in get_args(InteractionType):
            raise ValueError("engagement interaction must be a supported literal")


@dataclass(frozen=True, slots=True)
class ContentPerformance:
    content_id: str
    sessions: int
    avg_session_duration_seconds: float
    avg_scroll_depth: float
    interaction_rate: float
    engagement_score: float


@dataclass(frozen=True, slots=True)
class EngagementSummary:
    total_sessions: int
    avg_session_duration_seconds: float
    avg_scroll_depth: float
    peak_hours: tuple[int, ...]
    trend: dict[str, float]
    user_segments: dict[str, int]
    content_performance: dict[str, ContentPerformance]


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
        reference_time = _finite_number(reference_time, "reference_time")
        window_seconds = _finite_number(window_seconds, "window_seconds", minimum=0)
        if window_seconds <= 0.0:
            raise ValueError("window_seconds must be positive")
        start = reference_time - window_seconds
        scoped = tuple(
            _ScoredEvent(event, _event_score(event))
            for event in events
            if start <= event.timestamp <= reference_time
        )
        if not scoped:
            return EngagementSummary(0, 0.0, 0.0, (), {}, {}, {})

        return EngagementSummary(
            total_sessions=len(scoped),
            avg_session_duration_seconds=sum(
                item.event.session_duration_seconds for item in scoped
            )
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
            name: (sum(values) / len(values) if values else 0.0)
            for name, values in buckets.items()
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
