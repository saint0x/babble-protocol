from __future__ import annotations

import math
from dataclasses import asdict, replace
from pathlib import Path
from typing import Literal, cast

import pytest

from babble_algorithms.execution import AlgorithmExecutor
from babble_algorithms.ranking_time import timestamp_nanos
from babble_algorithms.temporal import (
    ContentTimeClass,
    EngagementWindow,
    TemporalInput,
    TemporalScorer,
)
from babble_algorithms.temporal_types import (
    TEMPORAL_PROVIDER,
    TemporalItem,
    TemporalOutput,
    TemporalProvider,
    TemporalRequest,
    TemporalResult,
)
from babble_algorithms.wire import MAX_ID, PROTOCOL, Json, decode, object_value
from babble_algorithms.worker import encode, handle

from .ranking_assertions import assert_json_close
from .test_worker import error, exchange, frame, health, judge


def item(index: int = 0) -> dict[str, Json]:
    return {
        "object_id": f"obj_{index:064x}",
        "published_at": "2026-09-30T00:00:00Z",
        "content_class": "discussion",
        "quality_score": 0.5,
        "tags": [],
        "engagement": {
            "total_views": 0,
            "recent_views": 0,
            "total_interactions": 10,
            "recent_interactions": 5,
        },
    }


def envelope(items: list[Json] | None = None) -> dict[str, Json]:
    return {
        "protocol": PROTOCOL,
        "id": 10,
        "method": "temporal",
        "request": {
            "reference_time": "2026-09-30t01:00:00.000z",
            "items": [item()] if items is None else items,
        },
    }


def run(value: Json) -> TemporalResult:
    response = handle(frame(value), AlgorithmExecutor())
    assert response.error is None
    assert isinstance(response.result, TemporalResult)
    return response.result


def test_full_formula_without_invented_view_counts() -> None:
    result = run(envelope())
    assert result.provider == TEMPORAL_PROVIDER
    assert result.reference_time == "2026-09-30t01:00:00.000z"
    (score,) = result.scores
    assert score.object_id == item()["object_id"]
    assert score.age_hours == 1.0
    assert score.recency == 1.0
    assert score.time_sensitivity == 0.68
    velocity = 0.38 * 0.5 * math.exp(-1 / 168)
    decay = 0.1 + 0.16 * 0.68 - 0.12 * 0.5 - 0.1 * velocity
    assert score.engagement_velocity == pytest.approx(velocity)
    assert score.decay_rate == pytest.approx(decay)
    assert score.survival_score == min(1, math.exp(-decay / 24) + 0.22 * velocity)
    entry = item()
    entry["published_at"] = "2026-09-27T01:00:00Z"
    score = run(envelope([entry])).scores[0]
    velocity = 0.38 * 0.5 * math.exp(-72 / 168)
    decay = 0.1 + 0.16 * 0.68 - 0.12 * 0.5 - 0.1 * velocity
    assert score.survival_score == pytest.approx(0.62 * math.exp(-decay * 3) + 0.22 * velocity)


@pytest.mark.parametrize("size", [0, 1, 200])
def test_batch_preserves_input_order_and_exact_wire_fields(size: int) -> None:
    entries: list[Json] = [item(index) for index in reversed(range(size))]
    (response,) = exchange(frame(envelope(entries)))
    assert response["error"] is None
    body = object_value(response["result"])
    assert set(body) == {"provider", "reference_time", "scores"}
    assert body["provider"] == asdict(TEMPORAL_PROVIDER)
    scores = body["scores"]
    assert isinstance(scores, list)
    assert [object_value(score)["object_id"] for score in scores] == [
        object_value(entry)["object_id"] for entry in entries
    ]
    for score in scores:
        assert set(object_value(score)) == {
            "object_id",
            "age_hours",
            "recency",
            "decay_rate",
            "time_sensitivity",
            "engagement_velocity",
            "survival_score",
        }


@pytest.mark.parametrize(
    "boundary,base,next_base",
    [
        (2, 1.0, 0.82),
        (24, 0.82, 0.62),
        (72, 0.62, 0.42),
        (168, 0.42, 0.23),
        (720, 0.23, 0.1),
    ],
)
@pytest.mark.parametrize(
    "content_class,weight,sensitivity",
    [
        ("news", 1.2, 0.92),
        ("discussion", 1.0, 0.68),
        ("analysis", 0.82, 0.5),
        ("tutorial", 0.66, 0.28),
        ("reference", 0.45, 0.12),
    ],
)
def test_all_class_weights_and_inclusive_age_boundaries(
    boundary: int,
    base: float,
    next_base: float,
    content_class: str,
    weight: float,
    sensitivity: float,
) -> None:
    scorer = TemporalScorer()
    typed = ContentTimeClass(content_class)
    assert scorer.recency(boundary, typed) == min(1, base * weight)
    assert scorer.recency(math.nextafter(boundary, math.inf), typed) == min(1, next_base * weight)
    assert scorer.time_sensitivity(typed, ()) == sensitivity
    entry = item()
    entry["content_class"] = content_class
    assert run(envelope([entry])).scores[0].time_sensitivity == sensitivity


@pytest.mark.parametrize(
    "tags,expected",
    [
        ((), 0.68),
        (("BREAKING",), 0.86),
        (("TIME-SENSITIVE",), 0.86),
        (("EVERGREEN",), 0.5),
        (("REFERENCE",), 0.5),
        (("breaking", "time-sensitive", "evergreen", "reference"), 0.68),
        (("irrelevant",) * 32, 0.68),
        (("\U0001f600" * 16,), 0.68),
    ],
)
def test_tags(tags: tuple[str, ...], expected: float) -> None:
    entry = item()
    entry["tags"] = list(tags)
    assert run(envelope([entry])).scores[0].time_sensitivity == pytest.approx(expected)


def test_sensitivity_and_decay_clamps() -> None:
    scorer = TemporalScorer()
    assert scorer.time_sensitivity(ContentTimeClass.NEWS, ("breaking",)) == 1
    assert scorer.time_sensitivity(ContentTimeClass.REFERENCE, ("evergreen",)) == 0
    assert scorer.decay_rate(quality_score=1, engagement_velocity=1, time_sensitivity=0) == 0.01
    assert scorer.decay_rate(quality_score=0, engagement_velocity=0, time_sensitivity=1) == 0.26


@pytest.mark.parametrize(
    "published,reference,age",
    [
        ("9999-12-31T23:59:59.999999998Z", "9999-12-31T23:59:59.999999999Z", 1 / 3.6e12),
        ("0000-02-29T00:00:00Z", "0000-03-01T00:00:00Z", 24),
        ("2026-09-30T01:00:00+01:00", "2026-09-30T00:00:00Z", 0),
        ("2026-09-30T01:00:00Z", "2026-09-30T00:00:00Z", 0),
        ("2016-12-31T23:59:60Z", "2017-01-01T00:00:00Z", 1 / 3.6e12),
        ("2026-09-30T00:00:00Z", "2026-09-30T02:00:00.000000001Z", 2 + 1 / 3.6e12),
    ],
)
def test_precise_age_and_future_clamp(published: str, reference: str, age: float) -> None:
    entry = item()
    entry["published_at"] = published
    request = envelope([entry])
    object_value(request["request"])["reference_time"] = reference
    result = run(request)
    assert result.reference_time == reference
    score = result.scores[0]
    assert score.age_hours == age
    if age == 0:
        assert score.engagement_velocity == 0
    if age > 2:
        assert score.recency == 0.82


def test_entire_timestamp_range_is_finite() -> None:
    entry = item()
    entry["published_at"] = "0000-01-01T00:00:00+23:59"
    request = envelope([entry])
    reference = "9999-12-31T23:59:59-23:59"
    object_value(request["request"])["reference_time"] = reference
    score = run(request).scores[0]
    assert (
        score.age_hours
        == (timestamp_nanos(reference) - timestamp_nanos(str(entry["published_at"]))) / 3.6e12
    )
    assert score.survival_score == 0
    assert score.age_hours <= 87_658_248.0


@pytest.mark.parametrize(
    "scope,key,value",
    [
        ("envelope", "unknown", "PRIVATE_SECRET"),
        ("request", "unknown", "PRIVATE_SECRET"),
        ("item", "unknown", "PRIVATE_SECRET"),
        ("engagement", "unknown", "PRIVATE_SECRET"),
        ("item", "object_id", "obj_" + "A" * 64),
        ("item", "object_id", "obj_" + "0" * 63),
        ("item", "content_class", "PRIVATE_SECRET"),
        ("item", "content_class", True),
        ("item", "quality_score", True),
        ("item", "quality_score", -0.01),
        ("item", "quality_score", 1.01),
        ("item", "quality_score", 10**400),
        ("item", "quality_score", float("nan")),
        ("item", "quality_score", float("inf")),
        ("item", "tags", ["x"] * 33),
        ("item", "tags", ["\U0001f600" * 17]),
        ("item", "tags", [False]),
        ("item", "tags", "breaking"),
        ("request", "items", {}),
        ("item", "engagement", []),
        ("engagement", "total_views", True),
        ("engagement", "total_views", -1),
        ("engagement", "total_views", 1.0),
        ("engagement", "total_views", MAX_ID + 1),
        ("engagement", "total_views", 10**400),
        ("engagement", "recent_views", 1),
        ("engagement", "recent_interactions", 11),
    ],
)
def test_rejects_invalid_fields_privately(scope: str, key: str, value: Json) -> None:
    entry = item()
    request = envelope([entry])
    targets = {
        "envelope": request,
        "request": object_value(request["request"]),
        "item": entry,
        "engagement": object_value(entry["engagement"]),
    }
    targets[scope][key] = value
    response, healthy = exchange(frame(request) + frame(health()))
    error(response)
    assert "PRIVATE_SECRET" not in str(response)
    assert object_value(healthy["result"])["temporal_provider"] == asdict(TEMPORAL_PROVIDER)


@pytest.mark.parametrize(
    "invalid",
    [
        "",
        "PRIVATE_SECRET",
        "2026-02-29T00:00:00Z",
        "2026-09-30T24:00:00Z",
        "2026-09-30T00:00:00",
        "10000-01-01T00:00:00Z",
        "2026-09-30T00:00:00+24:00",
        "2026-09-30T00:00:60Z",
        "2026-09-30T00:00:00Zjunk",
        False,
        0,
        None,
        "2026-09-30 00:00:00Z",
        "2026-09-30_00:00:00Z",
        "2026-09-30\n00:00:00Z",
    ],
)
@pytest.mark.parametrize("field", ["reference_time", "published_at"])
def test_rejects_malformed_times(invalid: Json, field: str) -> None:
    entry = item()
    request = envelope([entry])
    target = entry if field == "published_at" else object_value(request["request"])
    target[field] = invalid
    response = handle(frame(request), AlgorithmExecutor())
    assert response.error is not None and response.error.code == "invalid_request"


def test_duplicate_ids_keys_and_oversized_batch() -> None:
    for entries in ([item(), item()], [item(i) for i in range(201)]):
        response = handle(frame(envelope(cast(list[Json], entries))), AlgorithmExecutor())
        assert response.error is not None and response.error.code == "invalid_request"
    raw = frame(envelope()).replace(
        b'"quality_score":0.5', b'"quality_score":0.5,"quality_score":1'
    )
    response = handle(raw, AlgorithmExecutor())
    assert response.error is not None and response.error.code == "invalid_request"


@pytest.mark.parametrize("raw", [b"1e999", b'"\\ud800"'])
def test_decoder_rejects_overflow_and_invalid_unicode(raw: bytes) -> None:
    data = frame(envelope()).replace(b'"quality_score":0.5', b'"quality_score":' + raw)
    response = handle(data, AlgorithmExecutor())
    assert response.error is not None and response.error.code == "invalid_request"


@pytest.mark.parametrize(
    "scope,key",
    [
        ("request", "reference_time"),
        ("request", "items"),
        ("item", "object_id"),
        ("item", "published_at"),
        ("item", "content_class"),
        ("item", "quality_score"),
        ("item", "tags"),
        ("item", "engagement"),
        ("engagement", "total_views"),
        ("engagement", "recent_views"),
        ("engagement", "total_interactions"),
        ("engagement", "recent_interactions"),
    ],
)
def test_all_contract_fields_are_required(scope: str, key: str) -> None:
    entry = item()
    request = envelope([entry])
    targets = {
        "request": object_value(request["request"]),
        "item": entry,
        "engagement": object_value(entry["engagement"]),
    }
    del targets[scope][key]
    response = handle(frame(request), AlgorithmExecutor())
    assert response.error is not None and response.error.code == "invalid_request"


def test_maximum_safe_counts_and_zero_counts() -> None:
    for count in (0, MAX_ID):
        entry = item()
        entry["engagement"] = dict.fromkeys(
            ("total_views", "recent_views", "total_interactions", "recent_interactions"), count
        )
        expected = 0 if count == 0 else math.exp(-1 / 168)
        assert run(envelope([entry])).scores[0].engagement_velocity == pytest.approx(expected)


@pytest.mark.parametrize("invalid", [True, -1, 1.0, MAX_ID + 1])
def test_domain_engagement_rejects_invalid_counts(invalid: int) -> None:
    with pytest.raises(ValueError):
        _ = EngagementWindow(total_views=invalid)
    with pytest.raises(ValueError):
        _ = EngagementWindow(recent_interactions=1)


@pytest.mark.parametrize("invalid", [True, float("nan"), float("inf"), -math.inf, 10**400])
def test_domain_public_methods_reject_nonfinite_or_bool(invalid: float) -> None:
    scorer = TemporalScorer()
    domain = TemporalInput("test", 0)
    with pytest.raises(ValueError):
        _ = replace(domain, published_at=invalid)
    with pytest.raises(ValueError):
        _ = replace(domain, quality_score=invalid)
    with pytest.raises(ValueError):
        _ = scorer.score(domain, reference_time=invalid)
    with pytest.raises(ValueError):
        _ = scorer.score_at_age(domain, age_hours=invalid)
    with pytest.raises(ValueError):
        _ = scorer.recency(invalid, ContentTimeClass.NEWS)
    with pytest.raises(ValueError):
        _ = scorer.engagement_velocity(EngagementWindow(), invalid)
    for field in ("quality_score", "engagement_velocity", "time_sensitivity"):
        values = dict.fromkeys(("quality_score", "engagement_velocity", "time_sensitivity"), 0.5)
        values[field] = invalid
        with pytest.raises(ValueError):
            _ = scorer.decay_rate(**values)


def test_domain_rejects_negative_age_bad_tags_and_timestamp_overflow() -> None:
    scorer = TemporalScorer()
    domain = TemporalInput("test", -1e308)
    with pytest.raises(ValueError):
        _ = scorer.score(domain, reference_time=1e308)
    with pytest.raises(ValueError):
        _ = scorer.recency(-1, ContentTimeClass.NEWS)
    with pytest.raises(ValueError):
        _ = scorer.engagement_velocity(EngagementWindow(), -1)
    with pytest.raises(ValueError):
        _ = replace(domain, tags=("x" * 65,))
    with pytest.raises(ValueError):
        _ = scorer.time_sensitivity(ContentTimeClass.NEWS, ("\ud800",))
    with pytest.raises(ValueError):
        _ = scorer.time_sensitivity(cast(ContentTimeClass, cast(object, "unknown")), ())
    assert scorer.score(TemporalInput("future", 2), reference_time=1).age_hours == 0


def test_temporal_request_dtos_reject_invalid_direct_values() -> None:
    valid = TemporalItem(
        object_id="obj_" + "0" * 64,
        published_at="2026-09-30T00:00:00Z",
        content_class=ContentTimeClass.DISCUSSION,
        quality_score=0.5,
        tags=(),
        engagement=EngagementWindow(),
    )
    _ = TemporalRequest("2026-09-30T01:00:00Z", (valid,))

    with pytest.raises(ValueError):
        _ = TemporalItem(
            "obj_short",
            valid.published_at,
            valid.content_class,
            valid.quality_score,
            valid.tags,
            valid.engagement,
        )
    with pytest.raises(ValueError):
        _ = TemporalItem(
            valid.object_id,
            "2026-09-30",
            valid.content_class,
            valid.quality_score,
            valid.tags,
            valid.engagement,
        )
    with pytest.raises(ValueError):
        _ = TemporalItem(
            valid.object_id,
            valid.published_at,
            cast(ContentTimeClass, cast(object, "discussion")),
            valid.quality_score,
            valid.tags,
            valid.engagement,
        )
    with pytest.raises(ValueError):
        _ = TemporalItem(
            valid.object_id,
            valid.published_at,
            valid.content_class,
            math.nan,
            valid.tags,
            valid.engagement,
        )
    with pytest.raises(ValueError):
        _ = TemporalItem(
            valid.object_id,
            valid.published_at,
            valid.content_class,
            valid.quality_score,
            cast(tuple[str, ...], cast(object, ["breaking"])),
            valid.engagement,
        )
    with pytest.raises(ValueError):
        _ = TemporalItem(
            valid.object_id,
            valid.published_at,
            valid.content_class,
            valid.quality_score,
            valid.tags,
            cast(EngagementWindow, object()),
        )

    with pytest.raises(ValueError):
        _ = TemporalRequest("2026-09-30", ())
    with pytest.raises(ValueError):
        _ = TemporalRequest(
            "2026-09-30T01:00:00Z",
            cast(tuple[TemporalItem, ...], cast(object, [valid])),
        )
    with pytest.raises(ValueError):
        _ = TemporalRequest("2026-09-30T01:00:00Z", (valid, valid))
    with pytest.raises(ValueError):
        _ = TemporalRequest("2026-09-30T01:00:00Z", (valid,) * 201)


def test_temporal_result_dtos_reject_invalid_direct_values() -> None:
    output = TemporalOutput(
        "obj_" + "0" * 64,
        age_hours=1.0,
        recency=1.0,
        decay_rate=0.1,
        time_sensitivity=0.5,
        engagement_velocity=0.2,
        survival_score=0.9,
    )
    result = TemporalResult(TEMPORAL_PROVIDER, "2026-09-30T01:00:00Z", (output,))
    assert result.scores == (output,)

    with pytest.raises(ValueError, match="provider"):
        _ = TemporalProvider(
            cast(Literal["babble-python"], cast(object, "other")), "temporal-v1", "1"
        )
    with pytest.raises(ValueError, match="object_id"):
        _ = replace(output, object_id="obj_short")
    with pytest.raises(ValueError, match="finite"):
        _ = replace(output, age_hours=math.nan)
    with pytest.raises(ValueError, match="finite"):
        _ = replace(output, recency=1.01)
    with pytest.raises(ValueError, match="finite"):
        _ = replace(output, decay_rate=0.0)
    with pytest.raises(ValueError, match="TemporalProvider"):
        _ = TemporalResult(cast(TemporalProvider, object()), result.reference_time, result.scores)
    with pytest.raises(ValueError, match="RFC3339"):
        _ = TemporalResult(TEMPORAL_PROVIDER, "2026-09-30", result.scores)
    with pytest.raises(ValueError, match="bounded tuple"):
        _ = TemporalResult(
            TEMPORAL_PROVIDER,
            result.reference_time,
            cast(tuple[TemporalOutput, ...], cast(object, [output])),
        )
    with pytest.raises(ValueError, match="TemporalOutput"):
        _ = TemporalResult(
            TEMPORAL_PROVIDER,
            result.reference_time,
            (cast(TemporalOutput, object()),),
        )
    with pytest.raises(ValueError, match="unique"):
        _ = TemporalResult(TEMPORAL_PROVIDER, result.reference_time, (output, output))
    with pytest.raises(ValueError, match="bounded tuple"):
        _ = TemporalResult(TEMPORAL_PROVIDER, result.reference_time, (output,) * 201)


@pytest.mark.parametrize("invalid", [-0.01, 1.01])
def test_domain_rejects_outside_unit_interval(invalid: float) -> None:
    scorer = TemporalScorer()
    domain = TemporalInput("test", 0)
    with pytest.raises(ValueError):
        _ = replace(domain, quality_score=invalid)
    for field in ("quality_score", "engagement_velocity", "time_sensitivity"):
        values = dict.fromkeys(("quality_score", "engagement_velocity", "time_sensitivity"), 0.5)
        values[field] = invalid
        with pytest.raises(ValueError):
            _ = scorer.decay_rate(**values)
    score = scorer.score(domain, reference_time=0)
    with pytest.raises(ValueError):
        _ = replace(score, survival_score=invalid)


def test_mixed_worker_stream_and_failure_privacy(monkeypatch: pytest.MonkeyPatch) -> None:
    responses = exchange(frame(envelope()) + frame(health()) + frame(judge("spam")))
    assert all(response["error"] is None for response in responses)

    def broken_temporal(_self: AlgorithmExecutor, _request: TemporalRequest) -> TemporalResult:
        raise RuntimeError("PRIVATE_SECRET")

    monkeypatch.setattr(AlgorithmExecutor, "temporal", broken_temporal)
    response = handle(frame(envelope()), AlgorithmExecutor())
    assert response.error is not None and response.error.code == "algorithm_failure"
    assert b"PRIVATE_SECRET" not in encode(response)


def test_canonical_rust_temporal_corpus() -> None:
    path = Path(__file__).parents[2] / "fixtures/protocol/v1/fixtures.json"
    fixtures = object_value(decode(path.read_bytes().rstrip() + b"\n"))
    corpus = object_value(fixtures["temporal_scoring"])
    assert corpus["model"] == TEMPORAL_PROVIDER.model
    assert corpus["version"] == TEMPORAL_PROVIDER.version
    cases = corpus["cases"]
    assert isinstance(cases, list) and cases
    for raw in cases:
        case = object_value(raw)
        expected = object_value(case["result"])
        expected["provider"] = {
            "provider": "babble-python",
            "model": "temporal-v1",
            "version": "1",
        }
        request = envelope()
        request["request"] = case["request"]
        response = handle(frame(request), AlgorithmExecutor())
        assert response.error is None, case["name"]
        actual = object_value(decode(encode(response)))["result"]
        assert_json_close(actual, expected)
