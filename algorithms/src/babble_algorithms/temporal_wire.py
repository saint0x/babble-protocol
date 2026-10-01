"""Strict temporal request validation within the shared bounded wire envelope."""

import re

from babble_algorithms.boundary import MAX_ID, InvalidRequest, Json, object_value, string
from babble_algorithms.ranking_time import timestamp_nanos
from babble_algorithms.temporal import ContentTimeClass, EngagementWindow, finite_number
from babble_algorithms.temporal_types import TemporalItem, TemporalRequest


def _record(value: Json, fields: str) -> dict[str, Json]:
    result = object_value(value)
    if set(result) != set(fields.split()):
        raise InvalidRequest("missing or unknown temporal fields")
    return result


def _timestamp(value: Json) -> str:
    original = string(value)
    if len(original) < 11 or original[10] not in "Tt":
        raise InvalidRequest("invalid temporal timestamp separator")
    try:
        _ = timestamp_nanos(original)
    except ValueError:
        raise InvalidRequest("invalid temporal timestamp") from None
    return original


def _array(value: Json, maximum: int) -> list[Json]:
    if not isinstance(value, list) or len(value) > maximum:
        raise InvalidRequest("expected a bounded temporal array")
    return value


def _count(value: Json) -> int:
    if type(value) is not int or not 0 <= value <= MAX_ID:
        raise InvalidRequest("expected a nonnegative safe integer")
    return value


def _item(value: Json) -> TemporalItem:
    obj = _record(value, "object_id published_at content_class quality_score tags engagement")
    identity = string(obj["object_id"], 68)
    if re.fullmatch(r"obj_[0-9a-f]{64}", identity) is None:
        raise InvalidRequest("invalid temporal object ID")
    published_at = _timestamp(obj["published_at"])
    tags = tuple(string(tag, 64) for tag in _array(obj["tags"], 32))
    engagement = _record(
        obj["engagement"], "total_views recent_views total_interactions recent_interactions"
    )
    try:
        content_class = ContentTimeClass(string(obj["content_class"]))
        quality_score = finite_number(obj["quality_score"], minimum=0, maximum=1)
        window = EngagementWindow(**{key: _count(value) for key, value in engagement.items()})
    except ValueError:
        raise InvalidRequest("invalid temporal item") from None
    return TemporalItem(identity, published_at, content_class, quality_score, tags, window)


def parse_temporal_request(value: Json) -> TemporalRequest:
    obj = _record(value, "reference_time items")
    reference_time = _timestamp(obj["reference_time"])
    items = tuple(_item(item) for item in _array(obj["items"], 200))
    if len({item.object_id for item in items}) != len(items):
        raise InvalidRequest("temporal items must have unique IDs")
    return TemporalRequest(reference_time, items)
