from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from typing import Literal

from babble_algorithms.types import clamp_score

InteractionType = Literal["view", "expand", "react", "reply", "share", "save", "surface_open"]


@dataclass(frozen=True, slots=True)
class EngagementEvent:
    user_id: str
    content_id: str
    timestamp: float
    session_duration_seconds: float
    scroll_depth: float
    interaction: InteractionType = "view"


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


class EngagementAnalyzer:
    def summarize(
        self,
        events: tuple[EngagementEvent, ...],
        *,
        reference_time: float,
        window_seconds: float,
    ) -> EngagementSummary:
        if window_seconds <= 0.0:
            raise ValueError("window_seconds must be positive")
        start = reference_time - window_seconds
        scoped = tuple(event for event in events if start <= event.timestamp <= reference_time)
        if not scoped:
            return EngagementSummary(0, 0.0, 0.0, (), {}, {}, {})

        return EngagementSummary(
            total_sessions=len(scoped),
            avg_session_duration_seconds=sum(event.session_duration_seconds for event in scoped)
            / len(scoped),
            avg_scroll_depth=sum(event.scroll_depth for event in scoped) / len(scoped),
            peak_hours=self._peak_hours(scoped),
            trend=self._trend(scoped, start, window_seconds),
            user_segments=self._segments(scoped),
            content_performance=self._content_performance(scoped),
        )

    def _peak_hours(self, events: tuple[EngagementEvent, ...]) -> tuple[int, ...]:
        counts: dict[int, int] = defaultdict(int)
        for event in events:
            hour = int((event.timestamp % 86_400) // 3_600)
            counts[hour] += 1
        average = sum(counts.values()) / max(1, len(counts))
        return tuple(sorted(hour for hour, count in counts.items() if count > average))

    def _trend(
        self,
        events: tuple[EngagementEvent, ...],
        start: float,
        window_seconds: float,
    ) -> dict[str, float]:
        period = window_seconds / 6.0
        buckets: dict[str, list[float]] = {f"period_{index + 1}": [] for index in range(6)}
        for event in events:
            index = min(5, max(0, int((event.timestamp - start) / period)))
            buckets[f"period_{index + 1}"].append(_event_score(event))
        return {
            name: (sum(values) / len(values) if values else 0.0)
            for name, values in buckets.items()
        }

    def _segments(self, events: tuple[EngagementEvent, ...]) -> dict[str, int]:
        by_user: dict[str, list[float]] = defaultdict(list)
        for event in events:
            by_user[event.user_id].append(_event_score(event))

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
        self, events: tuple[EngagementEvent, ...]
    ) -> dict[str, ContentPerformance]:
        by_content: dict[str, list[EngagementEvent]] = defaultdict(list)
        for event in events:
            by_content[event.content_id].append(event)

        result: dict[str, ContentPerformance] = {}
        for content_id, values in by_content.items():
            sessions = len(values)
            interaction_count = sum(1 for event in values if event.interaction != "view")
            result[content_id] = ContentPerformance(
                content_id=content_id,
                sessions=sessions,
                avg_session_duration_seconds=sum(
                    event.session_duration_seconds for event in values
                )
                / sessions,
                avg_scroll_depth=sum(event.scroll_depth for event in values) / sessions,
                interaction_rate=interaction_count / sessions,
                engagement_score=sum(_event_score(event) for event in values) / sessions,
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
