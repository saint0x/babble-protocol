"""Strict, bounded input models for the local algorithm worker wire protocol."""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Literal, TypeAlias

from babble_algorithms.boundary import (
    MAX_ARRAY_ITEMS,
    MAX_DEPTH,
    MAX_ENTRIES,
    MAX_ID,
    MAX_NODES,
    MAX_SUBJECT_BYTES,
    MAX_TEXT_BYTES,
    InvalidRequest,
    Json,
    decode_json_frame,
    json_value,
    object_value,
    string,
    validate_tree,
)
from babble_algorithms.judgment import JudgmentDefinition
from babble_algorithms.moderation import ModerationContext, ModerationPolicy

if TYPE_CHECKING:
    from babble_algorithms.agreement_wire import SourceAgreementInput
    from babble_algorithms.ranking_types import RankingRequest
    from babble_algorithms.temporal_types import TemporalRequest

PROTOCOL = "babble.algorithms.v1"
MAX_LINE_BYTES = 4 * 1024 * 1024
MAX_JUDGMENT_LINE_BYTES = 1024 * 1024
MAX_JUDGMENT_NODES = 4096

__all__ = (
    "DEFINITIONS",
    "MAX_ARRAY_ITEMS",
    "MAX_DEPTH",
    "MAX_ENTRIES",
    "MAX_ID",
    "MAX_JUDGMENT_LINE_BYTES",
    "MAX_JUDGMENT_NODES",
    "MAX_LINE_BYTES",
    "MAX_NODES",
    "MAX_SUBJECT_BYTES",
    "MAX_TEXT_BYTES",
    "PROTOCOL",
    "Definition",
    "ErrorCode",
    "HealthRequest",
    "InvalidRequest",
    "Json",
    "JudgeRequest",
    "JudgmentRequest",
    "JudgmentState",
    "Parameters",
    "RankRequest",
    "Relation",
    "TemporalWorkerRequest",
    "UnsupportedDefinition",
    "decode",
    "json_value",
    "object_value",
    "parse_request",
    "request_id",
    "string",
    "validate_tree",
)

Definition: TypeAlias = (
    JudgmentDefinition
    | Literal["babble.judgment.content_analysis.v1", "babble.judgment.moderation.v1",
              "babble.judgment.source_agreement.v1"]
)
Relation: TypeAlias = Literal["supports", "contradicts", "related"]
ErrorCode: TypeAlias = Literal["invalid_request", "unsupported_definition", "algorithm_failure"]
DEFINITIONS: tuple[Definition, ...] = (
    "babble.judgment.spam.v1",
    "babble.judgment.evidence_quality.v1",
    "babble.judgment.relevance.v1",
    "babble.judgment.relationship.v1",
    "babble.judgment.content_analysis.v1",
    "babble.judgment.moderation.v1",
    "babble.judgment.source_agreement.v1",
)


class UnsupportedDefinition(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class Parameters:
    query: str = ""
    relation: Relation = "related"
    context: ModerationContext = field(default_factory=ModerationContext)
    policy: ModerationPolicy = field(default_factory=ModerationPolicy)


@dataclass(frozen=True, slots=True)
class JudgmentState:
    subject: str
    context: dict[str, Json]
    text: str
    source_text: str | None = None
    target_text: str | None = None
    source_agreement: SourceAgreementInput | None = None


@dataclass(frozen=True, slots=True)
class JudgmentRequest:
    definition: Definition
    state: JudgmentState
    parameters: Parameters


@dataclass(frozen=True, slots=True)
class HealthRequest:
    id: int


@dataclass(frozen=True, slots=True)
class JudgeRequest:
    id: int
    request: JudgmentRequest


@dataclass(frozen=True, slots=True)
class RankRequest:
    id: int
    request: RankingRequest


@dataclass(frozen=True, slots=True)
class TemporalWorkerRequest:
    id: int
    request: TemporalRequest


def decode(line: bytes) -> Json:
    return decode_json_frame(line, max_line_bytes=MAX_LINE_BYTES)


def _fields(value: dict[str, Json], required: set[str], optional: set[str] | None = None) -> None:
    if not required <= value.keys() or value.keys() - required - (optional or set()):
        raise InvalidRequest("missing or unknown request fields")


def request_id(value: Json) -> int | None:
    candidate = value.get("id") if isinstance(value, dict) else None
    return candidate if type(candidate) is int and 1 <= candidate <= MAX_ID else None


def parse_request(
    value: Json,
) -> HealthRequest | JudgeRequest | RankRequest | TemporalWorkerRequest:
    envelope = object_value(value)
    identity = request_id(envelope)
    if identity is None:
        raise InvalidRequest("id must be a positive safe integer")
    if envelope.get("protocol") != PROTOCOL:
        raise InvalidRequest("unsupported protocol")
    method = envelope.get("method")
    if method == "health":
        _fields(envelope, {"protocol", "id", "method"})
        return HealthRequest(identity)
    if method == "rank":
        from babble_algorithms.ranking_wire import parse_ranking_request

        _fields(envelope, {"protocol", "id", "method", "request"})
        return RankRequest(identity, parse_ranking_request(envelope["request"]))
    if method == "temporal":
        from babble_algorithms.temporal_wire import parse_temporal_request

        _fields(envelope, {"protocol", "id", "method", "request"})
        return TemporalWorkerRequest(identity, parse_temporal_request(envelope["request"]))
    if method != "judge":
        raise InvalidRequest("unsupported method")
    validate_tree(value, MAX_JUDGMENT_NODES)
    _fields(envelope, {"protocol", "id", "method", "request"})
    body = object_value(envelope["request"])
    _fields(body, {"definition", "state", "parameters"})
    definition = string(body["definition"], MAX_SUBJECT_BYTES, nonblank=True)
    state = object_value(body["state"])
    _fields(state, {"subject", "context"})
    subject = string(state["subject"], MAX_SUBJECT_BYTES, nonblank=True)
    context = object_value(state["context"])
    text = string(context.get("text"), nonblank=True)
    parameters = object_value(body["parameters"])
    if definition not in DEFINITIONS:
        raise UnsupportedDefinition
    supported = definition
    source_text: str | None = None
    target_text: str | None = None
    agreement = None
    if supported == "babble.judgment.source_agreement.v1":
        from babble_algorithms.agreement_wire import MAX_ID_BYTES, parse_source_agreement

        _ = string(subject, MAX_ID_BYTES, nonblank=True)
        agreement = parse_source_agreement(context.get("source_agreement"))
    if supported == "babble.judgment.relationship.v1" and (
        "source_text" in context or "target_text" in context
    ):
        source_text = string(context.get("source_text"), nonblank=True)
        target_text = string(context.get("target_text"), nonblank=True)
    return JudgeRequest(
        identity,
        JudgmentRequest(
            supported,
            JudgmentState(subject, context, text, source_text, target_text, agreement),
            _parameters(supported, parameters),
        ),
    )


def _number(value: Json, maximum: float = MAX_ID) -> float:
    if type(value) not in (int, float) or not isinstance(value, (int, float)):
        raise InvalidRequest("expected a number")
    if not 0 <= value <= maximum or not math.isfinite(value):
        raise InvalidRequest("number is outside its finite range")
    return float(value)


def _count(value: Json) -> int:
    if type(value) is not int or not 0 <= value <= MAX_ID:
        raise InvalidRequest("expected a nonnegative safe integer")
    return value


def _parameters(definition: Definition, values: dict[str, Json]) -> Parameters:
    if definition == "babble.judgment.relevance.v1":
        _fields(values, set(), {"query"})
        return Parameters(query=string(values.get("query", "")))
    if definition == "babble.judgment.relationship.v1":
        _fields(values, set(), {"relation"})
        relation = string(values.get("relation", "related"))
        if relation not in ("supports", "contradicts", "related"):
            raise InvalidRequest("unsupported relationship relation")
        return Parameters(relation=relation)
    if definition == "babble.judgment.moderation.v1":
        _fields(values, set(), {"context", "policy"})
        context = object_value(values.get("context", {}))
        policy = object_value(values.get("policy", {}))
        _fields(
            context,
            set(),
            {
                "repeated_messages",
                "account_age_days",
                "reports",
                "similar_recent_posts",
                "external_links",
            },
        )
        _fields(
            policy, set(), {"spam_limit", "quality_warn", "safety_remove", "coordination_limit"}
        )
        return Parameters(
            context=ModerationContext(
                repeated_messages=_count(context.get("repeated_messages", 0)),
                account_age_days=_number(context.get("account_age_days", 30.0)),
                reports=_count(context.get("reports", 0)),
                similar_recent_posts=_count(context.get("similar_recent_posts", 0)),
                external_links=_count(context.get("external_links", 0)),
            ),
            policy=ModerationPolicy(
                spam_limit=_number(policy.get("spam_limit", 0.7), 1),
                quality_warn=_number(policy.get("quality_warn", 0.32), 1),
                safety_remove=_number(policy.get("safety_remove", 0.82), 1),
                coordination_limit=_number(policy.get("coordination_limit", 0.65), 1),
            ),
        )
    _fields(values, set())
    return Parameters()
