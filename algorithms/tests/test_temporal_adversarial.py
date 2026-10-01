"""Independent boundary and batch invariants for the real temporal execution path."""

from __future__ import annotations

from dataclasses import asdict

import pytest
from test_temporal import envelope, item, run
from test_worker import error, exchange, frame, health

from babel_algorithms.execution import AlgorithmExecutor
from babel_algorithms.temporal_types import TEMPORAL_PROVIDER
from babel_algorithms.wire import MAX_ID, Json, object_value
from babel_algorithms.worker import handle


@pytest.mark.parametrize("year", [0, 1, 400, 1970, 2026, 9999])
@pytest.mark.parametrize("hours,base,next_base", [
    (2, 1.0, 0.82), (24, 0.82, 0.62), (72, 0.62, 0.42),
    (168, 0.42, 0.23), (720, 0.23, 0.1),
])
@pytest.mark.parametrize("nanos", [0, 1, 999])
def test_wire_recency_boundary_is_independent_of_epoch_and_offset(
    year: int, hours: int, base: float, next_base: float, nanos: int,
) -> None:
    entry = item()
    entry["published_at"] = f"{year:04d}-01-01T01:00:00+01:00"
    request = envelope([entry])
    days, hour = divmod(hours, 24)
    reference = f"{year:04d}-01-{days + 1:02d}T{hour:02d}:00:00.{nanos:09d}Z"
    object_value(request["request"])["reference_time"] = reference
    result = run(request)
    assert result.reference_time == reference
    score, = result.scores
    assert score.age_hours == (hours * 3_600_000_000_000 + nanos) / 3_600_000_000_000
    # An approximate age comparison would hide a one-nanosecond bucket error.
    assert score.recency == (base if nanos == 0 else next_base)


@pytest.mark.parametrize("published,reference", [
    ("2017-01-01T00:59:60+01:00", "2017-01-01T00:00:00Z"),
    ("2016-12-31T22:59:60-01:00", "2017-01-01T00:00:00Z"),
    ("0000-03-01T00:59:60.123+01:00", "0000-03-01T00:00:00Z"),
    ("2000-03-01T00:59:60.999999999999+01:00", "2000-03-01T00:00:00Z"),
])
def test_leap_second_uses_utc_month_boundary(published: str, reference: str) -> None:
    entry = item()
    entry["published_at"] = published
    request = envelope([entry])
    object_value(request["request"])["reference_time"] = reference
    score, = run(request).scores
    assert score.age_hours == 1 / 3_600_000_000_000
    assert score.engagement_velocity > 0


@pytest.mark.parametrize("invalid", [
    "2016-12-31T23:59:60+01:00",
    "2016-12-31T23:59:60-01:00",
    "2017-01-02T00:59:60+01:00",
    "0000-02-28T23:59:60Z",
    "1900-02-29T00:00:00Z",
    "2026-09-30T00:00:00+00:60",
    "2026-09-30T00:00:00+00:00:01",
    "\uff12\uff10\uff12\uff16-09-30T00:00:00Z",
    "2026-09-30T00:00:00.\u0661Z",
    "2026-09-30T00:00:00Z\n",
])
def test_empty_batches_still_validate_the_reference(invalid: str) -> None:
    request = envelope([])
    object_value(request["request"])["reference_time"] = invalid
    response = handle(frame(request), AlgorithmExecutor())
    assert response.result is None
    assert response.error is not None and response.error.code == "invalid_request"


@pytest.mark.parametrize("reference", [
    "0000-01-01t00:00:00.000000001+23:59",
    "9999-12-31T23:59:59.999999999-23:59",
    "2026-09-30t00:00:00.123456789999999-00:00",
    "2016-12-31T23:59:60z",
])
def test_empty_batches_keep_exact_reference_and_provenance(reference: str) -> None:
    request = envelope([])
    object_value(request["request"])["reference_time"] = reference
    response, = exchange(frame(request))
    assert response["error"] is None
    assert response["result"] == {
        "provider": asdict(TEMPORAL_PROVIDER), "reference_time": reference, "scores": [],
    }


@pytest.mark.parametrize("fraction,age,recency", [
    ("000000000999999999", 2.0, 1.0),
    ("000000001000000000", 2 + 1 / 3_600_000_000_000, 0.82),
    ("999999999999999999", 2 + 999_999_999 / 3_600_000_000_000, 0.82),
])
def test_subnanosecond_digits_truncate_without_rounding_into_next_bucket(
    fraction: str, age: float, recency: float,
) -> None:
    request = envelope()
    reference = f"2026-09-30T02:00:00.{fraction}Z"
    object_value(request["request"])["reference_time"] = reference
    result = run(request)
    assert result.reference_time == reference
    assert result.scores[0].age_hours == age
    assert result.scores[0].recency == recency


@pytest.mark.parametrize("tag,expected", [
    ("TIME-SEN\u017fITIVE", 0.86),
    ("brea\u212aing", 0.86),
    ("time-sens\u0131tive", 0.68),
    ("break\u0130ng", 0.68),
    ("\uff42\uff52\uff45\uff41\uff4b\uff49\uff4e\uff47", 0.68),
    (" breaking", 0.68),
    ("breaking\u0000", 0.68),
    ("reference\n", 0.68),
])
def test_unicode_casefold_does_not_imply_compatibility_or_whitespace_normalization(
    tag: str, expected: float,
) -> None:
    entry = item()
    entry["tags"] = [tag]
    assert run(envelope([entry])).scores[0].time_sensitivity == pytest.approx(expected)


def test_partition_order_and_failed_requests_do_not_change_scores_in_one_worker() -> None:
    entries: list[Json] = []
    classes = ["news", "discussion", "analysis", "tutorial", "reference"]
    for index in range(200):
        entry = item(index)
        entry["content_class"] = classes[index % len(classes)]
        entry["quality_score"] = (index % 11) / 10
        tags: list[Json] = ["\U0001f642" * 16 for _ in range(31)]
        tags.append("breaking" if index % 2 == 0 else "evergreen")
        entry["tags"] = tags
        entry["engagement"] = {
            "total_views": 0,
            "recent_views": 0,
            "total_interactions": MAX_ID,
            "recent_interactions": MAX_ID - index,
        }
        entries.append(entry)

    invalid = envelope([])
    object_value(invalid["request"])["private_history"] = "PRIVATE_SENTINEL"
    requests = [envelope(entries), invalid, health(), envelope([])]
    reversed_entries = list(reversed(entries))
    requests.extend(envelope(reversed_entries[start:start + 37]) for start in range(0, 200, 37))
    responses = exchange(b"".join(frame(request) for request in requests))
    assert len(responses) == len(requests)
    error(responses[1])
    assert "PRIVATE_SENTINEL" not in str(responses)
    assert object_value(responses[2]["result"])["temporal_provider"] == asdict(TEMPORAL_PROVIDER)
    assert object_value(responses[3]["result"])["scores"] == []
    baseline = object_value(responses[0]["result"])["scores"]
    assert isinstance(baseline, list) and len(baseline) == 200
    reassembled: list[Json] = []
    for response in responses[4:]:
        assert response["error"] is None
        result = object_value(response["result"])
        assert result["provider"] == asdict(TEMPORAL_PROVIDER)
        assert result["reference_time"] == "2026-09-30t01:00:00.000z"
        scores = result["scores"]
        assert isinstance(scores, list)
        reassembled.extend(scores)
    assert reassembled == list(reversed(baseline))
