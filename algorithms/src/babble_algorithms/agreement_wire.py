"""Strict source agreement wire boundary; the pure analyzer owns scoring."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Literal

from babble_algorithms.consensus import ConsensusSource, ConsensusState
from babble_algorithms.wire import InvalidRequest, Json, object_value, string

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


def _fields(value: dict[str, Json], expected: set[str]) -> None:
    if value.keys() != expected:
        raise InvalidRequest("missing or unknown agreement fields")


def _number(value: Json, maximum: float) -> float:
    if type(value) not in (int, float) or not isinstance(value, (int, float)):
        raise InvalidRequest("score must be a number")
    if not 0 <= value <= maximum or not math.isfinite(value):
        raise InvalidRequest("score outside finite unit interval")
    return float(value)


def _timestamp(value: Json) -> float:
    if type(value) not in (int, float) or not isinstance(value, (int, float)):
        raise InvalidRequest("timestamp must be a number")
    if not MIN_TIMESTAMP <= value < MAX_TIMESTAMP or not math.isfinite(value):
        raise InvalidRequest("timestamp outside supported range")
    return float(value)


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
        _fields(source, {
            "source_id", "kind", "text", "timestamp", "quality_score", "evidence_score",
            "user_id", "vote", "is_context",
        })
        identity = string(source["source_id"], MAX_ID_BYTES, nonblank=True)
        if identity in identities:
            raise InvalidRequest("duplicate source ID")
        identities.add(identity)
        kind = source["kind"]
        if kind not in (
            "official_docs", "research_paper", "technical_blog", "community_wiki",
            "forum_post", "social_media", "context",
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
        sources.append(ConsensusSource(
            source_id=identity, kind=kind, text=text, timestamp=timestamp,
            quality_score=_number(source["quality_score"], 1),
            evidence_score=_number(source["evidence_score"], 1),
            user_id=user_id, vote=vote, is_context=is_context,
        ))
    return SourceAgreementInput(reference_time, previous_score, tuple(sources))
