from __future__ import annotations

import copy
from dataclasses import asdict
from typing import Literal, cast

import pytest

from babble_algorithms.agreement_wire import (
    SourceAgreementInput,
    SourceAgreementOutput,
    parse_source_agreement,
)
from babble_algorithms.consensus import ConsensusAnalyzer, ConsensusSource, ConsensusState
from babble_algorithms.execution import AlgorithmExecutor
from babble_algorithms.wire import DEFINITIONS, Json, json_value, object_value
from babble_algorithms.worker import encode, handle

from .test_worker import error, exchange, frame, health, judge, output, result


def source(index: int = 0) -> dict[str, Json]:
    return {
        "source_id": f"source-{index}",
        "kind": "research_paper",
        "text": "The measured result is reproducible.",
        "timestamp": 100.0,
        "quality_score": 0.8,
        "evidence_score": 0.7,
        "user_id": f"user-{index}",
        "vote": 0.9,
        "is_context": False,
    }


def agreement(count: int = 2) -> dict[str, Json]:
    return {
        "reference_time": 200.0,
        "previous_score": None,
        "sources": [source(index) for index in range(count)],
    }


def request(value: Json) -> dict[str, Json]:
    return judge("source_agreement", context={"source_agreement": value})


@pytest.mark.parametrize("count", [0, 1, 2, 200])
def test_real_worker_agreement_matches_pure_analyzer(count: int) -> None:
    value = agreement(count)
    responses = exchange(frame(health()) + frame(request(value)))
    supported = result(responses[0])["supported_definitions"]
    assert supported == list(DEFINITIONS)
    assert len(DEFINITIONS) == 7
    actual = output(responses[1])
    parsed = parse_source_agreement(value)
    expected = object_value(
        json_value(
            asdict(
                ConsensusAnalyzer().evaluate(
                    "obj_test",
                    parsed.sources,
                    reference_time=parsed.reference_time,
                    previous_score=parsed.previous_score,
                )
            )
        )
    )
    for key, expected_value in expected.items():
        assert actual[key] == expected_value
    assert actual["reference_time"] == 200.0
    assert actual["source_ids"] == [f"source-{index}" for index in range(count)]
    assert actual["confidence"] == 0.0
    assert actual["confidence_status"] == "uncalibrated"
    assert actual["limitations"]
    assert len(object_value(actual["user_contributions"])) == count


def test_source_agreement_dtos_reject_invalid_direct_values() -> None:
    first = ConsensusSource(
        "source-1",
        "research_paper",
        "The measured result is reproducible.",
        100.0,
        quality_score=0.8,
        evidence_score=0.7,
        user_id="user-1",
        vote=0.9,
    )
    second = ConsensusSource(
        "source-2",
        "technical_blog",
        "The measurement has independent confirmation.",
        120.0,
        user_id="user-2",
        vote=0.8,
    )
    valid_input = SourceAgreementInput(200.0, 0.5, (first, second))
    assert valid_input.sources == (first, second)

    valid_output = SourceAgreementOutput(
        kind="source_agreement",
        confidence=0.0,
        confidence_status="uncalibrated",
        reference_time=200.0,
        source_ids=("source-1", "source-2"),
        content_id="obj_test",
        consensus_score=0.5,
        reliability_score=0.6,
        validation_count=2,
        state=ConsensusState.PROVISIONAL,
        temporal_weight=0.7,
        term_agreement=0.4,
        fact_agreement=0.3,
        user_contributions={"user-1": 0.8},
        limitations=("lexical limitation",),
    )
    assert valid_output.user_contributions == {"user-1": 0.8}

    anonymous_vote = ConsensusSource(
        "source-3", "research_paper", "Text", 100.0, user_id=None, vote=0.5
    )
    with pytest.raises(ValueError, match="reference_time"):
        _ = SourceAgreementInput(True, None, ())
    with pytest.raises(ValueError, match="previous_score"):
        _ = SourceAgreementInput(200.0, 1.1, ())
    with pytest.raises(ValueError, match="bounded tuple"):
        _ = SourceAgreementInput(200.0, None, cast(tuple[ConsensusSource, ...], cast(object, [])))
    with pytest.raises(ValueError, match="ConsensusSource"):
        _ = SourceAgreementInput(200.0, None, (cast(ConsensusSource, object()),))
    with pytest.raises(ValueError, match="unique"):
        _ = SourceAgreementInput(200.0, None, (first, first))
    with pytest.raises(ValueError, match="reference_time"):
        _ = SourceAgreementInput(50.0, None, (first,))
    with pytest.raises(ValueError, match="anonymous"):
        _ = SourceAgreementInput(200.0, None, (anonymous_vote,))

    with pytest.raises(ValueError, match="kind"):
        _ = SourceAgreementOutput(
            cast(Literal["source_agreement"], cast(object, "agreement")),
            0.0,
            "uncalibrated",
            200.0,
            (),
            "obj",
            0.5,
            0.5,
            0,
            ConsensusState.INSUFFICIENT,
            0.0,
            0.0,
            0.0,
            {},
            ("limitation",),
        )
    with pytest.raises(ValueError, match="confidence_status"):
        _ = SourceAgreementOutput(
            "source_agreement",
            0.0,
            cast(Literal["uncalibrated"], cast(object, "calibrated")),
            200.0,
            (),
            "obj",
            0.5,
            0.5,
            0,
            ConsensusState.INSUFFICIENT,
            0.0,
            0.0,
            0.0,
            {},
            ("limitation",),
        )
    with pytest.raises(ValueError, match="score"):
        _ = SourceAgreementOutput(
            "source_agreement",
            0.0,
            "uncalibrated",
            200.0,
            (),
            "obj",
            float("nan"),
            0.5,
            0,
            ConsensusState.INSUFFICIENT,
            0.0,
            0.0,
            0.0,
            {},
            ("limitation",),
        )
    with pytest.raises(ValueError, match="unique"):
        _ = SourceAgreementOutput(
            "source_agreement",
            0.0,
            "uncalibrated",
            200.0,
            ("source-1", "source-1"),
            "obj",
            0.5,
            0.5,
            0,
            ConsensusState.INSUFFICIENT,
            0.0,
            0.0,
            0.0,
            {},
            ("limitation",),
        )
    with pytest.raises(ValueError, match="user_contributions"):
        _ = SourceAgreementOutput(
            "source_agreement",
            0.0,
            "uncalibrated",
            200.0,
            (),
            "obj",
            0.5,
            0.5,
            0,
            ConsensusState.INSUFFICIENT,
            0.0,
            0.0,
            0.0,
            cast(dict[str, float], cast(object, ())),
            ("limitation",),
        )
    with pytest.raises(ValueError, match="blank"):
        _ = SourceAgreementOutput(
            "source_agreement",
            0.0,
            "uncalibrated",
            200.0,
            (),
            "obj",
            0.5,
            0.5,
            0,
            ConsensusState.INSUFFICIENT,
            0.0,
            0.0,
            0.0,
            {},
            ("",),
        )


def test_previous_score_and_repeated_voters_reach_analyzer() -> None:
    value = agreement(2)
    first, second = source(0), source(1)
    second["user_id"] = first["user_id"]
    second["vote"] = 0.1
    value["sources"] = [first, second]
    value["previous_score"] = 0.9
    actual = output(exchange(frame(request(value)))[0])
    expected_input = parse_source_agreement(value)
    expected = ConsensusAnalyzer().evaluate(
        "obj_test",
        expected_input.sources,
        reference_time=200.0,
        previous_score=0.9,
    )
    assert actual["state"] == expected.state
    assert actual["user_contributions"] == expected.user_contributions
    empty = agreement(0)
    empty["previous_score"] = 0.9
    assert output(exchange(frame(request(empty)))[0])["state"] == "revoked"


@pytest.mark.parametrize(
    "field,bad",
    [
        ("source_id", ""),
        ("source_id", "x" * 513),
        ("source_id", "\u00e9" * 257),
        ("user_id", " "),
        ("user_id", "x" * 513),
        ("kind", "unknown"),
        ("text", "x" * 65537),
        ("text", "\u00e9" * 32769),
        ("text", None),
        ("timestamp", True),
        ("timestamp", "100"),
        ("timestamp", 201),
        ("timestamp", -62167219201),
        ("timestamp", 253402300800),
        ("timestamp", float("nan")),
        ("timestamp", float("inf")),
        ("quality_score", True),
        ("quality_score", -0.1),
        ("evidence_score", 1.1),
        ("vote", False),
        ("vote", -1),
        ("is_context", 1),
        ("user_id", None),
    ],
)
def test_invalid_sources_are_sanitized_and_next_frame_recovers(field: str, bad: Json) -> None:
    item = source()
    item[field] = bad
    value = agreement(0)
    value["sources"] = [item]
    responses = exchange(frame(request(value)) + frame(request(agreement(0))))
    error(responses[0])
    assert output(responses[1])["validation_count"] == 0


@pytest.mark.parametrize("field", list(source()))
def test_every_source_field_is_required_including_nullable_fields(field: str) -> None:
    item = source()
    del item[field]
    value = agreement(0)
    value["sources"] = [item]
    error(exchange(frame(request(value)))[0])


def test_unknown_missing_duplicate_and_batch_bounds() -> None:
    invalid: list[Json] = [None, [], {}, agreement(201)]
    for field in ("reference_time", "previous_score", "sources"):
        value = agreement()
        del value[field]
        invalid.append(value)
    value = agreement()
    value["extra"] = "private"
    invalid.append(value)
    item = source()
    item["extra"] = "private"
    invalid.append({**agreement(), "sources": [item]})
    invalid.append({**agreement(), "sources": [source(), source()]})
    for field in ("reference_time", "previous_score"):
        for bad in (True, "1", -62167219201, 253402300800, float("inf")):
            invalid.append({**agreement(), field: bad})
    for response in exchange(b"".join(frame(request(value)) for value in invalid)):
        error(response)


def test_utf8_per_source_total_and_identifier_limits() -> None:
    value = agreement(0)
    items = [source(index) for index in range(8)]
    for item in items:
        item["text"] = "\u00e9" * 32768
        item["user_id"] = None
        item["vote"] = None
    items[0]["source_id"] = "\u00e9" * 256
    value["sources"] = [item for item in items]
    assert output(exchange(frame(request(value)))[0])["validation_count"] == 8
    oversized = copy.deepcopy(value)
    oversized["sources"] = [*items, {**source(9), "text": "a"}]
    error(exchange(frame(request(oversized)))[0])


def test_supported_timestamp_extremes_and_required_nulls() -> None:
    value = agreement(0)
    value["reference_time"] = 253402300799.0
    value["sources"] = [{**source(), "timestamp": -62167219200.0, "user_id": None, "vote": None}]
    actual = output(exchange(frame(request(value)))[0])
    assert actual["temporal_weight"] == 0
    assert actual["user_contributions"] == {}


def test_domain_failure_is_sanitized_and_executor_recovers(monkeypatch: pytest.MonkeyPatch) -> None:
    def fail(*_args: object, **_kwargs: object) -> None:
        raise RuntimeError("PRIVATE source text traceback")

    executor = AlgorithmExecutor()
    with monkeypatch.context() as patch:
        patch.setattr(ConsensusAnalyzer, "evaluate", fail)
        response = handle(frame(request(agreement())), executor)
        assert response.error is not None and response.error.code == "algorithm_failure"
        assert b"PRIVATE" not in encode(response)
    assert handle(frame(request(agreement())), executor).error is None


def test_raw_duplicate_nonfinite_and_wrong_parameters() -> None:
    valid = frame(request(agreement()))
    invalid = [
        valid.replace(b'"reference_time":200.0', b'"reference_time":1,"reference_time":200'),
        valid.replace(b'"quality_score":0.8', b'"quality_score":1e9999'),
        valid.replace(b'"parameters":{}', b'"parameters":{"previous_score":0.9}'),
    ]
    for response in exchange(b"".join(invalid)):
        error(response)
