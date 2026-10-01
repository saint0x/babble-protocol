from __future__ import annotations

import json
import math
from typing import TypeAlias, TypeGuard, cast

MAX_TEXT_BYTES = 128 * 1024
MAX_SUBJECT_BYTES = 4096
MAX_DEPTH = 16
MAX_ENTRIES = 64
MAX_ARRAY_ITEMS = 256
MAX_NODES = 200000
MAX_ID = 9007199254740991

Json: TypeAlias = bool | int | float | str | list["Json"] | dict[str, "Json"] | None


class InvalidRequest(ValueError):
    """Messages are fixed strings suitable for the public error envelope."""


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


def check_depth(text: str) -> None:
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


def decode_json_frame(
    line: bytes, *, max_line_bytes: int, max_text_bytes: int = MAX_TEXT_BYTES
) -> Json:
    if not line.endswith(b"\n") or len(line) > max_line_bytes:
        raise InvalidRequest("request must be a bounded newline-terminated frame")
    try:
        text = line.decode("utf-8", errors="strict")
        check_depth(text)
        raw = cast(object, json.loads(text, object_pairs_hook=_pairs, parse_constant=_constant))
        value = json_value(raw)
        validate_tree(value, max_text_bytes=max_text_bytes)
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
                _ = string(key, MAX_SUBJECT_BYTES)
            pending.extend((child, (*path, key)) for key, child in item.items())
        elif isinstance(item, list):
            if len(item) > MAX_ARRAY_ITEMS:
                raise InvalidRequest("JSON array entry count exceeds the limit")
            pending.extend((child, (*path, "[]")) for child in item)
        elif isinstance(item, str):
            _ = string(item, max_text_bytes)
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
