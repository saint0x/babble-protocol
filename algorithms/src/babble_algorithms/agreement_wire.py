"""Strict source agreement wire boundary; the pure analyzer owns scoring."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Literal, cast

from babble_algorithms.boundary import InvalidRequest, Json, object_value, string
from babble_algorithms.consensus import ConsensusSource, ConsensusState

MAX_SOURCES = 200
MAX_SOURCE_TEXT_BYTES = 64 * 1024
MAX_TOTAL_TEXT_BYTES = 512 * 1024
MAX_ID_BYTES = 512
MIN_TIMESTAMP = -62167219200.0
MAX_TIMESTAMP = 253402300800.0


@dataclass(frozen=True, slots=True)
class SourceAgreementInput:
    reference_time: float
    previous_score: float | None
    sources: tuple[ConsensusSource, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "reference_time", _timestamp_value(self.reference_time))
        if self.previous_score is not None:
            object.__setattr__(
                self, "previous_score", _number_value(self.previous_score, 1, "previous_score")
            )
        object.__setattr__(self, "sources", _sources(self.sources, self.reference_time))


@dataclass(frozen=True, slots=True)
class SourceAgreementOutput:
    kind: Literal["source_agreement"]
    confidence: float
    confidence_status: Literal["uncalibrated"]
    reference_time: float
    source_ids: tuple[str, ...]
    content_id: str
    consensus_score: float
    reliability_score: float
    validation_count: int
    state: ConsensusState
    temporal_weight: float
    term_agreement: float
    fact_agreement: float
    user_contributions: dict[str, float]
    limitations: tuple[str, ...]

    def __post_init__(self) -> None:
        if self.kind != "source_agreement":
            raise ValueError("source agreement output kind is invalid")
        if self.confidence_status != "uncalibrated":
            raise ValueError("source agreement confidence_status is invalid")
        object.__setattr__(self, "confidence", _number_value(self.confidence, 1, "confidence"))
        object.__setattr__(self, "reference_time", _timestamp_value(self.reference_time))
        object.__setattr__(self, "source_ids", _source_ids(self.source_ids))
        object.__setattr__(self, "content_id", _identifier(self.content_id, "content_id"))
        object.__setattr__(
            self, "consensus_score", _number_value(self.consensus_score, 1, "consensus_score")
        )
        object.__setattr__(
            self, "reliability_score", _number_value(self.reliability_score, 1, "reliability_score")
        )
        if type(self.validation_count) is not int or not 0 <= self.validation_count <= MAX_SOURCES:
            raise ValueError("validation_count must be a bounded integer")
        if type(self.state) is not ConsensusState:
            raise ValueError("state must be ConsensusState")
        object.__setattr__(
            self, "temporal_weight", _number_value(self.temporal_weight, 1, "temporal_weight")
        )
        object.__setattr__(
            self, "term_agreement", _number_value(self.term_agreement, 1, "term_agreement")
        )
        object.__setattr__(
            self, "fact_agreement", _number_value(self.fact_agreement, 1, "fact_agreement")
        )
        object.__setattr__(self, "user_contributions", _contributions(self.user_contributions))
        object.__setattr__(self, "limitations", _strings(self.limitations, "limitations"))


def _fields(value: dict[str, Json], expected: set[str]) -> None:
    if value.keys() != expected:
        raise InvalidRequest("missing or unknown agreement fields")


def _number_value(value: object, maximum: float, label: str) -> float:
    if type(value) not in (int, float) or not isinstance(value, (int, float)):
        raise ValueError(f"{label} must be a number")
    if not 0 <= value <= maximum or not math.isfinite(value):
        raise ValueError(f"{label} outside finite unit interval")
    return float(value)


def _timestamp_value(value: object) -> float:
    if type(value) not in (int, float) or not isinstance(value, (int, float)):
        raise ValueError("reference_time must be a number")
    if not MIN_TIMESTAMP <= value < MAX_TIMESTAMP or not math.isfinite(value):
        raise ValueError("reference_time outside supported range")
    return float(value)


def _number(value: Json, maximum: float) -> float:
    try:
        return _number_value(value, maximum, "score")
    except ValueError as error:
        raise InvalidRequest(str(error)) from None


def _timestamp(value: Json) -> float:
    try:
        return _timestamp_value(value)
    except ValueError as error:
        raise InvalidRequest(str(error)) from None


def _identifier(value: object, label: str) -> str:
    if not isinstance(value, str):
        raise ValueError(f"{label} must be a string")
    _ = string(value, MAX_ID_BYTES, nonblank=True)
    return value


def _sources(value: object, reference_time: float) -> tuple[ConsensusSource, ...]:
    if type(value) is not tuple:
        raise ValueError("sources must be a bounded tuple")
    values = cast(tuple[object, ...], value)
    if len(values) > MAX_SOURCES:
        raise ValueError("sources must be a bounded tuple")
    sources: list[ConsensusSource] = []
    identities: set[str] = set()
    total_bytes = 0
    for source in values:
        if not isinstance(source, ConsensusSource):
            raise ValueError("sources must contain ConsensusSource values")
        if source.source_id in identities:
            raise ValueError("source IDs must be unique")
        identities.add(source.source_id)
        total_bytes += len(source.text.encode("utf-8"))
        if total_bytes > MAX_TOTAL_TEXT_BYTES:
            raise ValueError("total source text exceeds byte limit")
        if source.timestamp > reference_time:
            raise ValueError("source timestamp must not exceed reference_time")
        if source.vote is not None and source.user_id is None:
            raise ValueError("anonymous votes are not accepted")
        sources.append(source)
    return tuple(sources)


def _source_ids(value: object) -> tuple[str, ...]:
    if type(value) is not tuple:
        raise ValueError("source_ids must be a bounded tuple")
    values = cast(tuple[object, ...], value)
    if len(values) > MAX_SOURCES:
        raise ValueError("source_ids must be a bounded tuple")
    ids: list[str] = []
    seen: set[str] = set()
    for source_id in values:
        identity = _identifier(source_id, "source_id")
        if identity in seen:
            raise ValueError("source_ids must be unique")
        seen.add(identity)
        ids.append(identity)
    return tuple(ids)


def _contributions(value: object) -> dict[str, float]:
    if type(value) is not dict:
        raise ValueError("user_contributions must be a dict")
    contributions: dict[str, float] = {}
    for user_id, contribution in cast(dict[object, object], value).items():
        contributions[_identifier(user_id, "user_id")] = _number_value(
            contribution, 1, "user_contributions"
        )
    return contributions


def _strings(value: object, label: str) -> tuple[str, ...]:
    if type(value) is not tuple:
        raise ValueError(f"{label} must be a tuple")
    strings: list[str] = []
    for item in cast(tuple[object, ...], value):
        strings.append(_identifier(item, label))
    return tuple(strings)


def parse_source_agreement(value: Json) -> SourceAgreementInput:
    body = object_value(value)
    _fields(body, {"reference_time", "previous_score", "sources"})
    reference_time = _timestamp(body["reference_time"])
    previous = body["previous_score"]
    previous_score = None if previous is None else _number(previous, 1)
    values = body["sources"]
    if not isinstance(values, list) or len(values) > MAX_SOURCES:
        raise InvalidRequest("sources must be a bounded array")
    sources: list[ConsensusSource] = []
    identities: set[str] = set()
    total_bytes = 0
    for value in values:
        source = object_value(value)
        _fields(
            source,
            {
                "source_id",
                "kind",
                "text",
                "timestamp",
                "quality_score",
                "evidence_score",
                "user_id",
                "vote",
                "is_context",
            },
        )
        identity = string(source["source_id"], MAX_ID_BYTES, nonblank=True)
        if identity in identities:
            raise InvalidRequest("duplicate source ID")
        identities.add(identity)
        kind = source["kind"]
        if kind not in (
            "official_docs",
            "research_paper",
            "technical_blog",
            "community_wiki",
            "forum_post",
            "social_media",
            "context",
        ):
            raise InvalidRequest("invalid source kind")
        text = string(source["text"], MAX_SOURCE_TEXT_BYTES)
        total_bytes += len(text.encode("utf-8"))
        if total_bytes > MAX_TOTAL_TEXT_BYTES:
            raise InvalidRequest("total source text exceeds byte limit")
        timestamp = _timestamp(source["timestamp"])
        if timestamp > reference_time:
            raise InvalidRequest("future source")
        user = source["user_id"]
        user_id = None if user is None else string(user, MAX_ID_BYTES, nonblank=True)
        vote_value = source["vote"]
        vote = None if vote_value is None else _number(vote_value, 1)
        if vote is not None and user_id is None:
            raise InvalidRequest("anonymous vote")
        is_context = source["is_context"]
        if type(is_context) is not bool:
            raise InvalidRequest("is_context must be boolean")
        sources.append(
            ConsensusSource(
                source_id=identity,
                kind=kind,
                text=text,
                timestamp=timestamp,
                quality_score=_number(source["quality_score"], 1),
                evidence_score=_number(source["evidence_score"], 1),
                user_id=user_id,
                vote=vote,
                is_context=is_context,
            )
        )
    return SourceAgreementInput(reference_time, previous_score, tuple(sources))
