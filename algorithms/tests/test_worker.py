from __future__ import annotations

import json
import random
import select
import subprocess
import sys
from dataclasses import asdict
from pathlib import Path
from typing import cast

import pytest

from babble_algorithms import worker as worker_module
from babble_algorithms.content import ContentAnalyzer
from babble_algorithms.execution import PROVIDER, AlgorithmExecutor
from babble_algorithms.judgment import LocalJudgmentProvider
from babble_algorithms.moderation import CommunityModerator, ModerationContext, ModerationPolicy
from babble_algorithms.ranking_types import RANKING_PROVIDER
from babble_algorithms.temporal_types import TEMPORAL_PROVIDER
from babble_algorithms.wire import (
    DEFINITIONS,
    MAX_ID,
    MAX_JUDGMENT_NODES,
    MAX_LINE_BYTES,
    MAX_TEXT_BYTES,
    PROTOCOL,
    Json,
    decode,
    json_value,
    object_value,
)
from babble_algorithms.worker import Response, encode, handle

from .ranking_assertions import assert_json_close


def frame(value: Json) -> bytes:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"


def health(identity: Json = 1) -> dict[str, Json]:
    return {"protocol": PROTOCOL, "id": identity, "method": "health"}


def judge(
    name: str,
    text: Json = "According to the study, this useful dataset supports replication.",
    parameters: Json = None,
    *,
    context: dict[str, Json] | None = None,
    subject: Json = "obj_test",
) -> dict[str, Json]:
    return {
        "protocol": PROTOCOL,
        "id": 2,
        "method": "judge",
        "request": {
            "definition": f"babble.judgment.{name}.v1",
            "state": {"subject": subject, "context": {"text": text, **(context or {})}},
            "parameters": {} if parameters is None else parameters,
        },
    }


def exchange(data: bytes) -> list[dict[str, Json]]:
    process = subprocess.run(
        [sys.executable, "-m", "babble_algorithms.worker"],
        input=data,
        capture_output=True,
        timeout=15,
    )
    assert process.returncode == 0, process.stderr.decode()
    assert process.stderr == b""
    lines = process.stdout.splitlines(keepends=True)
    assert all(line.endswith(b"\n") and len(line) <= MAX_LINE_BYTES for line in lines)
    responses: list[dict[str, Json]] = []
    for line in lines:
        response = cast(dict[str, Json], json.loads(line))
        assert set(response) == {"protocol", "id", "result", "error"}
        assert response["protocol"] == PROTOCOL
        assert (response["result"] is None) != (response["error"] is None)
        responses.append(response)
    return responses


def result(response: dict[str, Json]) -> dict[str, Json]:
    assert response["error"] is None, response
    value = response["result"]
    assert isinstance(value, dict)
    assert value["provider"] == asdict(PROVIDER)
    return value


def output(response: dict[str, Json]) -> dict[str, Json]:
    value = result(response)
    body = value["output"]
    assert isinstance(body, dict)
    assert body["confidence"] == value["confidence"]
    return body


def error(response: dict[str, Json], code: str = "invalid_request") -> None:
    value = response["error"]
    assert isinstance(value, dict)
    assert set(value) == {"code", "message"}
    assert value["code"] == code
    assert response["result"] is None


def test_health_and_all_definitions_are_real() -> None:
    names = [definition.split(".")[2] for definition in DEFINITIONS]
    requests = [judge(name, context={"source_agreement": {
        "reference_time": 0.0, "previous_score": None, "sources": [],
    }} if name == "source_agreement" else {}) for name in names]
    responses = exchange(frame(health()) + b"".join(frame(request) for request in requests))
    assert result(responses[0])["supported_definitions"] == list(DEFINITIONS)
    assert result(responses[0])["temporal_provider"] == asdict(TEMPORAL_PROVIDER)
    assert len(responses) == 8
    assert [output(response)["kind"] for response in responses[1:]] == [
        "probability",
        "bounded_score",
        "bounded_score",
        "relationship",
        "content_analysis",
        "moderation",
        "source_agreement",
    ]
    for response in responses[1:]:
        assert output(response)["limitations"]


def test_rust_wire_fixtures_match_real_worker() -> None:
    fixtures = cast(
        dict[str, Json],
        json.loads(
            (Path(__file__).parents[2] / "fixtures/algorithms/v1/fixtures.json").read_text()
        ),
    )
    requests = fixtures["requests"]
    expected = fixtures["responses"]
    assert isinstance(requests, list) and isinstance(expected, list)
    actual = exchange(b"".join(frame(request) for request in requests) + b"invalid\n")
    assert len(actual) == len(expected)
    expected_health = expected[0]
    assert isinstance(expected_health, dict)
    actual_definitions = result(actual[0])["supported_definitions"]
    expected_definitions = result(expected_health)["supported_definitions"]
    assert isinstance(actual_definitions, list) and isinstance(expected_definitions, list)
    assert set(str(item) for item in actual_definitions) == set(
        str(item) for item in expected_definitions
    )
    assert result(actual[0])["ranking_provider"] == asdict(RANKING_PROVIDER)
    assert result(expected_health)["ranking_provider"] == asdict(RANKING_PROVIDER)
    assert result(actual[0])["temporal_provider"] == asdict(TEMPORAL_PROVIDER)
    assert result(expected_health)["temporal_provider"] == asdict(TEMPORAL_PROVIDER)
    for response, fixture in zip(actual[1:], expected[1:], strict=True):
        body = response["result"]
        if isinstance(body, dict) and "ranked" in body:
            assert body["provider"] == asdict(RANKING_PROVIDER)
            assert set(body) == {"ranked", "trace", "diversity_trace", "provider"}
            assert_json_close(response, fixture)
        elif isinstance(body, dict) and "scores" in body:
            assert body["provider"] == asdict(TEMPORAL_PROVIDER)
            assert set(body) == {"scores", "reference_time", "provider"}
            assert_json_close(response, fixture)
        else:
            assert response == fixture


def test_local_provider_mapping_and_query_binding() -> None:
    text = "Study supports replication. Buy now: https://example.invalid"
    requests = [
        judge(name, text, {"query": "study absent"} if name == "relevance" else {})
        for name in ("spam", "evidence_quality", "relevance")
    ]
    responses = exchange(b"".join(frame(request) for request in requests))
    local = LocalJudgmentProvider()
    for definition, response in zip(DEFINITIONS[:3], responses, strict=True):
        assert definition in (
            "babble.judgment.spam.v1",
            "babble.judgment.evidence_quality.v1",
            "babble.judgment.relevance.v1",
        )
        expected = local.judge(definition, text, context="study absent")
        actual = output(response)
        assert actual["score"] == expected.score
        assert actual["confidence"] == expected.confidence
        assert actual["label"] == expected.label
        assert actual["reasons"] == list(expected.reasons)
    no_query = output(exchange(frame(judge("relevance", text, context={"query": "study"})))[0])
    assert no_query["score"] == 0


@pytest.mark.parametrize(
    "relation,expected", [("supports", 0.0), ("contradicts", 0.82), ("related", 0.82)]
)
def test_relationship_does_not_relabel_opposite_finding(relation: str, expected: float) -> None:
    request = judge(
        "relationship",
        "However, this contradicts the proposal.",
        {"relation": relation},
        context={"target": "A completely different claim"},
    )
    actual = output(exchange(frame(request))[0])
    assert actual["relation"] == relation
    assert actual["observed_relation"] == "contradicts"
    assert actual["score"] == expected
    assert actual["target_context_evaluated"] is False
    assert "No target comparison" in str(actual["limitations"])
    assert actual["marker_scope"] == "text"


def test_relationship_unknown_and_support_cases() -> None:
    requests = [
        judge("relationship", "ordinary text", {"relation": "supports"}),
        judge("relationship", "confirms the study", {"relation": "supports"}),
        judge("relationship", "confirms the study", {"relation": "contradicts"}),
    ]
    values = [output(r) for r in exchange(b"".join(frame(r) for r in requests))]
    assert [value["score"] for value in values] == [0, 0.72, 0]


def test_relationship_explicit_pair_keeps_target_markers_out_of_source() -> None:
    request = judge(
        "relationship",
        "source: ordinary text target: contradicts",
        {"relation": "contradicts"},
        context={"source_text": "ordinary text", "target_text": "contradicts"},
    )
    actual = output(exchange(frame(request))[0])
    assert actual["relation"] == "contradicts"
    assert actual["score"] == 0
    assert actual["observed_relation"] == "unknown"
    assert actual["marker_scope"] == "source_text"
    assert actual["target_context_evaluated"] is False


@pytest.mark.parametrize(
    "text",
    [
        "good useful supports",
        "bad harm hate",
        "neutral text",
        "!!!",
        "\u7814\u7a76 \u00e9tude \U0001f680 useful.",
    ],
)
def test_content_mapping(text: str) -> None:
    expected = ContentAnalyzer().analyze("obj_test", text)
    actual = output(exchange(frame(judge("content_analysis", text)))[0])
    assert actual["sentiment"] == (expected.sentiment_score + 1) / 2
    assert actual["topics"] == [topic for topic, score in expected.topics.items() if score > 0]
    assert actual["evidence_markers"] == list(expected.evidence.markers_found)
    assert actual["key_terms"] == list(expected.key_terms)
    assert actual["summary"] == (expected.summary or text)
    assert actual["properties"] == asdict(expected.properties)
    assert actual["confidence"] == 0
    assert actual["confidence_status"] == "uncalibrated"


@pytest.mark.parametrize(
    "text",
    [
        "tiny",
        "you should die idiot worthless trash kill attack",
        "secret cure",
        "According to the study, useful research works.",
    ],
)
def test_moderation_mapping(text: str) -> None:
    expected = CommunityModerator().analyze("obj_test", text)
    actual = output(exchange(frame(judge("moderation", text)))[0])
    assert actual["action"] == ("flag" if expected.action == "warn" else expected.action)
    assert actual["advisory_action"] == expected.action
    assert actual["safety"] == 1 - expected.scores.safety
    assert actual["spam"] == expected.scores.spam
    assert actual["quality"] == expected.scores.quality
    assert actual["coordination"] == expected.scores.coordination
    assert actual["misinformation"] == float("misinformation_pattern" in expected.flags)
    assert "not fact checking" in str(actual["limitations"])
    assert actual["confidence"] == 0
    assert actual["confidence_status"] == "uncalibrated"


def test_moderation_exposed_context_and_policy_are_used() -> None:
    context = ModerationContext(
        repeated_messages=3, reports=3, similar_recent_posts=2, external_links=2, account_age_days=1
    )
    policy = ModerationPolicy(
        spam_limit=0.3, quality_warn=0.1, safety_remove=0.9, coordination_limit=0.9
    )
    text = "Buy now guaranteed."
    expected = CommunityModerator(policy).analyze("obj_test", text, context=context)
    parameters = cast(Json, {"context": asdict(context), "policy": asdict(policy)})
    actual = output(exchange(frame(judge("moderation", text, parameters)))[0])
    assert actual["action"] == expected.action
    assert actual["coordination"] == expected.scores.coordination
    assert actual["spam"] == expected.scores.spam


@pytest.mark.parametrize("identity", [None, True, False, 0, -1, 1.0, "1", MAX_ID + 1, [], {}])
def test_invalid_ids_are_null(identity: Json) -> None:
    actual = exchange(frame(health(identity)))[0]
    error(actual)
    assert actual["id"] is None


def test_maximum_id_is_preserved() -> None:
    assert exchange(frame(health(MAX_ID)))[0]["id"] == MAX_ID


@pytest.mark.parametrize(
    "raw",
    [
        b"\n",
        b"null\n",
        b"[]\n",
        b"true\n",
        b"1\n",
        b"{}\n",
        b"not-json\n",
        b"\xff\n",
        b'{"protocol":"babble.algorithms.v1","id":1,"id":2,"method":"health"}\n',
        b'{"id":1,"nested":{"x":1,"x":2}}\n',
        b'{"id":1,"x":NaN}\n',
        b'{"id":1,"x":Infinity}\n',
        b'{"id":1,"x":-Infinity}\n',
        b'{"id":1,"x":1e9999}\n',
        b'{"id":1,"x":"\\ud800"}\n',
        b"{} {}\n",
        b"\xef\xbb\xbf{}\n",
        b'{"x":' + b"[" * 1000 + b"0" + b"]" * 1000 + b"}\n",
        b'{"x":' + b"9" * 5000 + b"}\n",
    ],
)
def test_bad_json_is_sanitized_and_next_frame_recovers(raw: bytes) -> None:
    responses = exchange(raw + frame(health(3)))
    assert len(responses) == 2
    error(responses[0])
    assert responses[0]["id"] is None
    assert responses[1]["id"] == 3
    result(responses[1])


@pytest.mark.parametrize(
    "payload",
    [
        {**health(), "unexpected": "PRIVATE_SECRET"},
        {**health(), "request": {}},
        {**health(), "method": "missing"},
        {**health(), "protocol": "wrong"},
        judge("spam", 3),
        judge("spam", True),
        judge("spam", []),
        judge("spam", " \n\t"),
        judge("spam", subject=" "),
        judge("spam", subject=False),
        judge("spam", subject="a" * 4097),
        judge("spam", "a" * (MAX_TEXT_BYTES + 1)),
        judge("spam", "\u00e9" * (MAX_TEXT_BYTES // 2 + 1)),
        judge("spam", parameters={"unknown": "PRIVATE_SECRET"}),
        judge("relevance", parameters={"query": False}),
        judge("relevance", parameters={"query": []}),
        judge("relationship", parameters={"relation": "unknown"}),
        judge("relationship", parameters={"relation": None}),
        judge("relationship", context={"source_text": "source"}),
        judge("relationship", context={"source_text": False, "target_text": "target"}),
        judge("moderation", parameters={"context": {"reports": -1}}),
        judge("moderation", parameters={"context": {"reports": True}}),
        judge("moderation", parameters={"context": {"reports": 1.5}}),
        judge("moderation", parameters={"context": {"account_age_days": -1}}),
        judge("moderation", parameters={"context": {"external_links": MAX_ID + 1}}),
        judge("moderation", parameters={"context": {"private": "PRIVATE_SECRET"}}),
        judge("moderation", parameters={"policy": {"quality_warn": "0.5"}}),
        judge("moderation", parameters={"policy": {"safety_remove": 1.01}}),
        judge("moderation", parameters={"policy": {"spam_limit": True}}),
        judge("moderation", parameters={"policy": None}),
        judge("spam", context={str(i): i for i in range(64)}),
        judge("spam", context={"array": cast(list[Json], [0] * 257)}),
        judge("spam", context={"array": cast(list[Json], [[0] * 256 for _ in range(16)])}),
        judge("spam", parameters=[]),
    ],
)
def test_invalid_shapes_and_limits(payload: Json) -> None:
    response = exchange(frame(payload))[0]
    error(response)
    assert "PRIVATE_SECRET" not in str(response)


def test_unknown_structural_fields_and_missing_fields() -> None:
    requests: list[Json] = []
    for target in ("envelope", "request", "state"):
        for operation in ("add", "remove"):
            request = judge("spam")
            body = cast(dict[str, Json], request["request"])
            state = cast(dict[str, Json], body["state"])
            selected = {"envelope": request, "request": body, "state": state}[target]
            if operation == "add":
                selected["unknown"] = 1
            else:
                del selected[next(iter(selected))]
            requests.append(request)
    for response in exchange(b"".join(frame(request) for request in requests)):
        error(response)


def test_unsupported_definition_is_distinct() -> None:
    response = exchange(frame(judge("unknown")))[0]
    error(response, "unsupported_definition")
    assert response["id"] == 2


def test_unicode_text_subject_and_exact_text_byte_limit() -> None:
    text = "\u00e9" * (MAX_TEXT_BYTES // 2)
    actual = output(
        exchange(frame(judge("content_analysis", text, subject="\U0001f680" * 1024)))[0]
    )
    assert actual["summary"] == text + "."


def test_depth_limit_counts_containers_and_ignores_braces_in_strings() -> None:
    nested: Json = '\\"{[ ]}'
    for _ in range(12):
        nested = [nested]
    valid = judge("spam", context={"nested": nested})
    invalid = judge("spam", context={"nested": [nested]})
    responses = exchange(frame(valid) + frame(invalid))
    output(responses[0])
    error(responses[1])


def test_line_limit_includes_newline() -> None:
    base = frame(health()).rstrip(b"\n")
    exact = base + b" " * (MAX_LINE_BYTES - len(base) - 1) + b"\n"
    result(exchange(exact)[0])
    responses = exchange(exact[:-1] + b" \n" + frame(health(3)))
    assert len(responses) == 1
    error(responses[0])
    assert responses[0]["id"] is None


@pytest.mark.parametrize(
    "data",
    [frame(health())[:-1], b'{"id":', b"a" * MAX_LINE_BYTES, b"a" * (MAX_LINE_BYTES + 1)],
    ids=["missing-newline", "truncated-json", "exact-limit-no-newline", "over-limit-no-newline"],
)
def test_truncated_and_oversized_frames_stop(data: bytes) -> None:
    responses = exchange(data)
    assert len(responses) == 1
    error(responses[0])
    assert responses[0]["id"] is None


def test_clean_eof_and_multiple_lines() -> None:
    assert exchange(b"") == []
    responses = exchange(b"".join(frame(health(i)) for i in range(1, 21)))
    assert [response["id"] for response in responses] == list(range(1, 21))


def test_seeded_invalid_byte_frames_recover_without_crashing() -> None:
    rng = random.Random(29)
    lines = [
        bytes(rng.randrange(0, 256) for _ in range(rng.randrange(1, 512))).replace(b"\n", b" ")
        + b"\n"
        for _ in range(100)
    ]
    responses = exchange(b"".join(lines) + frame(health()))
    assert len(responses) == 101
    for response in responses[:-1]:
        error(response)
    result(responses[-1])


def test_maximum_dotted_moderation_text_has_bounded_runtime() -> None:
    output(exchange(frame(judge("moderation", "a." * (MAX_TEXT_BYTES // 2))))[0])


def test_moderation_email_marker_survives_punctuation() -> None:
    with_email = CommunityModerator().analyze("id", "Contact name@example.test.")
    without_email = CommunityModerator().analyze("id", "Contact name.example.test.")
    assert with_email.scores.spam > without_email.scores.spam


@pytest.mark.parametrize("value", [-1, True, float("nan"), float("inf"), MAX_ID + 1])
def test_domain_context_rejects_invalid_account_age(value: float) -> None:
    with pytest.raises(ValueError):
        ModerationContext(account_age_days=value)


def test_response_is_flushed_before_stdin_closes() -> None:
    with subprocess.Popen(
        [sys.executable, "-m", "babble_algorithms.worker"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) as process:
        try:
            assert process.stdin is not None and process.stdout is not None
            process.stdin.write(frame(health()))
            process.stdin.flush()
            readable, _, _ = select.select([process.stdout], [], [], 5)
            assert readable
            response = cast(dict[str, Json], json.loads(process.stdout.readline()))
            result(response)
            process.stdin.close()
            assert process.wait(timeout=5) == 0
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)


def test_algorithm_failure_is_sanitized(monkeypatch: pytest.MonkeyPatch) -> None:
    def fail(_self: LocalJudgmentProvider, *_args: object, **_kwargs: object) -> None:
        raise RuntimeError("PRIVATE_SECRET traceback")

    monkeypatch.setattr(LocalJudgmentProvider, "judge", fail)
    response = handle(frame(judge("spam")), AlgorithmExecutor())
    assert response.error is not None and response.error.code == "algorithm_failure"
    assert b"PRIVATE_SECRET" not in encode(response)
    assert response.id == 2


@pytest.mark.parametrize("nodes", [MAX_JUDGMENT_NODES, MAX_JUDGMENT_NODES + 1])
def test_judgment_response_node_budget(monkeypatch: pytest.MonkeyPatch, nodes: int) -> None:
    response = handle(frame(judge("spam")), AlgorithmExecutor())
    body = object_value(json_value(asdict(response)))
    output = object_value(object_value(body["result"])["output"])
    probe: list[Json] = []
    output["probe"] = probe

    def count(value: Json) -> int:
        if isinstance(value, dict):
            return 1 + sum(count(item) for item in value.values())
        if isinstance(value, list):
            return 1 + sum(count(item) for item in value)
        return 1

    remaining = nodes - count(body)
    while remaining:
        if remaining == 1:
            probe.append(None)
            remaining -= 1
        else:
            children = min(256, remaining - 1)
            probe.append([None for _ in range(children)])
            remaining -= children + 1
    assert count(body) == nodes

    def serialize(value: Response) -> dict[str, Json]:
        return body if value is response else object_value(json_value(asdict(value)))

    monkeypatch.setattr(worker_module, "asdict", serialize)
    actual = object_value(decode(encode(response)))
    assert actual["id"] == response.id
    if nodes == MAX_JUDGMENT_NODES:
        assert actual == body
    else:
        error(actual, "algorithm_failure")
        assert actual["result"] is None


@pytest.mark.parametrize("value", [-1, True, 0.5, MAX_ID + 1])
def test_domain_context_rejects_invalid_counts(value: int) -> None:
    with pytest.raises(ValueError):
        ModerationContext(reports=value)


@pytest.mark.parametrize("value", [-1, True, float("nan"), float("inf"), 1.1])
def test_domain_policy_rejects_invalid_thresholds(value: float) -> None:
    with pytest.raises(ValueError):
        ModerationPolicy(spam_limit=value)
