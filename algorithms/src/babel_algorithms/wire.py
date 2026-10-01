"""Strict, bounded input models for the local algorithm worker wire protocol."""

from __future__ import annotations

import json
import math
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Literal, TypeAlias, TypeGuard

from babel_algorithms.judgment import JudgmentDefinition
from babel_algorithms.moderation import ModerationContext, ModerationPolicy

if TYPE_CHECKING:
    from babel_algorithms.agreement_wire import SourceAgreementInput
    from babel_algorithms.ranking_types import RankingRequest
    from babel_algorithms.temporal_types import TemporalRequest

PROTOCOL = "babel.algorithms.v1"
MAX_LINE_BYTES = 4 * 1024 * 1024
MAX_JUDGMENT_LINE_BYTES = 1024 * 1024
MAX_JUDGMENT_NODES = 4096
MAX_TEXT_BYTES = 128 * 1024
MAX_SUBJECT_BYTES = 4096
MAX_DEPTH = 16
MAX_ENTRIES = 64
MAX_ARRAY_ITEMS = 256
MAX_NODES = 200000
MAX_ID = 9007199254740991

Json: TypeAlias = bool | int | float | str | list["Json"] | dict[str, "Json"] | None
Definition: TypeAlias = (
    JudgmentDefinition
    | Literal["babel.judgment.content_analysis.v1", "babel.judgment.moderation.v1",
              "babel.judgment.source_agreement.v1"]
)
Relation: TypeAlias = Literal["supports", "contradicts", "related"]
ErrorCode: TypeAlias = Literal["invalid_request", "unsupported_definition", "algorithm_failure"]
DEFINITIONS: tuple[Definition, ...] = (
    "babel.judgment.spam.v1",
    "babel.judgment.evidence_quality.v1",
    "babel.judgment.relevance.v1",
    "babel.judgment.relationship.v1",
    "babel.judgment.content_analysis.v1",
    "babel.judgment.moderation.v1",
    "babel.judgment.source_agreement.v1",
)


class InvalidRequest(ValueError):
    """Messages are fixed strings suitable for the public error envelope."""


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


def _pairs(pairs: list[tuple[str, Json]]) -> dict[str, Json]:
    result: dict[str, Json] = {}
    for key, value in pairs:
        if key in result:
            raise InvalidRequest("duplicate JSON keys are not allowed")
        result[key] = value
    return result


def _constant(_value: str) -> Json:
    raise InvalidRequest("JSON numbers must be finite")


def _json_sequence(value: object) -> TypeGuard[list[object] | tuple[object, ...]]:
    return isinstance(value, (list, tuple))


def _json_object(value: object) -> TypeGuard[dict[object, object]]:
    return isinstance(value, dict)


def json_value(value: object) -> Json:
    """Validate untyped boundary values, including tuples produced by dataclasses.asdict."""
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if _json_sequence(value):
        return [json_value(item) for item in value]
    if _json_object(value):
        result: dict[str, Json] = {}
        for key, item in value.items():
            if not isinstance(key, str):
                raise InvalidRequest("JSON object keys must be strings")
            result[key] = json_value(item)
        return result
    raise InvalidRequest("unsupported JSON value")


def _check_depth(text: str) -> None:
    # Bound decoder recursion before constructing a JSON tree; braces in strings do not count.
    depth = 0
    in_string = False
    escaped = False
    for char in text:
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
        elif char == '"':
            in_string = True
        elif char in "[{":
            depth += 1
            if depth > MAX_DEPTH:
                raise InvalidRequest("JSON nesting exceeds the limit")
        elif char in "]}":
            depth -= 1


def decode(line: bytes) -> Json:
    if not line.endswith(b"\n") or len(line) > MAX_LINE_BYTES:
        raise InvalidRequest("request must be a bounded newline-terminated frame")
    try:
        text = line.decode("utf-8", errors="strict")
        _check_depth(text)
        raw: object = json.loads(text, object_pairs_hook=_pairs, parse_constant=_constant)
        value = json_value(raw)
        validate_tree(value)
        return value
    except (UnicodeError, json.JSONDecodeError, RecursionError, ValueError) as error:
        if isinstance(error, InvalidRequest):
            raise
        raise InvalidRequest("request must contain valid UTF-8 JSON") from None


def validate_tree(
    value: Json, max_nodes: int = MAX_NODES, *, max_text_bytes: int = MAX_TEXT_BYTES
) -> None:
    pending: list[tuple[Json, tuple[str, ...]]] = [(value, ())]
    nodes = 0
    while pending:
        item, path = pending.pop()
        nodes += 1
        if nodes > max_nodes:
            raise InvalidRequest("JSON value count exceeds the limit")
        if isinstance(item, dict):
            map_limit = 200 if path == ("result", "output", "user_contributions") else MAX_ENTRIES
            if len(item) > map_limit:
                raise InvalidRequest("JSON map entry count exceeds the limit")
            for key in item:
                string(key, MAX_SUBJECT_BYTES)
            pending.extend((child, (*path, key)) for key, child in item.items())
        elif isinstance(item, list):
            if len(item) > MAX_ARRAY_ITEMS:
                raise InvalidRequest("JSON array entry count exceeds the limit")
            pending.extend((child, (*path, "[]")) for child in item)
        elif isinstance(item, str):
            string(item, max_text_bytes)
        elif isinstance(item, float) and not math.isfinite(item):
            raise InvalidRequest("JSON numbers must be finite")


def string(value: Json, limit: int = MAX_TEXT_BYTES, *, nonblank: bool = False) -> str:
    if not isinstance(value, str):
        raise InvalidRequest("expected a string")
    try:
        size = len(value.encode("utf-8", errors="strict"))
    except UnicodeError:
        raise InvalidRequest("strings must contain Unicode scalar values") from None
    if size > limit or (nonblank and not value.strip()):
        raise InvalidRequest("string is blank or exceeds its byte limit")
    return value


def object_value(value: Json) -> dict[str, Json]:
    if not isinstance(value, dict):
        raise InvalidRequest("expected a JSON object")
    return value


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
        from babel_algorithms.ranking_wire import parse_ranking_request

        _fields(envelope, {"protocol", "id", "method", "request"})
        return RankRequest(identity, parse_ranking_request(envelope["request"]))
    if method == "temporal":
        from babel_algorithms.temporal_wire import parse_temporal_request

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
    if supported == "babel.judgment.source_agreement.v1":
        from babel_algorithms.agreement_wire import MAX_ID_BYTES, parse_source_agreement

        string(subject, MAX_ID_BYTES, nonblank=True)
        agreement = parse_source_agreement(context.get("source_agreement"))
    if supported == "babel.judgment.relationship.v1" and (
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
    if definition == "babel.judgment.relevance.v1":
        _fields(values, set(), {"query"})
        return Parameters(query=string(values.get("query", "")))
    if definition == "babel.judgment.relationship.v1":
        _fields(values, set(), {"relation"})
        relation = string(values.get("relation", "related"))
        if relation not in ("supports", "contradicts", "related"):
            raise InvalidRequest("unsupported relationship relation")
        return Parameters(relation=relation)
    if definition == "babel.judgment.moderation.v1":
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
