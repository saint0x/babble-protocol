from __future__ import annotations

import json
import math
import random
import select
import subprocess
import sys
from dataclasses import asdict, replace
from pathlib import Path
from typing import cast

import pytest

from babble_algorithms.execution import AlgorithmExecutor
from babble_algorithms.ranking import lens_reasons, rank
from babble_algorithms.ranking_time import parse_timestamp, timestamp_nanos
from babble_algorithms.ranking_types import (
    RANKING_PROVIDER,
    BuiltInLens,
    Candidate,
    DiversityPolicy,
    EvidenceSignals,
    LensStack,
    LensWeight,
    RankingRequest,
    Signals,
    SourceFloor,
)
from babble_algorithms.ranking_wire import parse_ranking_request
from babble_algorithms.types import CandidateSource, CandidateSourceContribution, ReputationSignals
from babble_algorithms.wire import (
    MAX_ARRAY_ITEMS,
    MAX_DEPTH,
    MAX_JUDGMENT_LINE_BYTES,
    MAX_JUDGMENT_NODES,
    MAX_LINE_BYTES,
    MAX_NODES,
    PROTOCOL,
    InvalidRequest,
    Json,
    decode,
    object_value,
)
from babble_algorithms.worker import encode, handle

from .ranking_assertions import assert_json_close


def candidate(index: int = 0, source: CandidateSource = "Following") -> Candidate:
    return Candidate(
        f"obj_{index:064x}",
        source,
        (CandidateSourceContribution(source, 1.0),),
        "2026-09-30T00:00:00Z",
        Signals(
            0.5,
            True,
            0.5,
            0.5,
            0.5,
            0.5,
            EvidenceSignals(1.0, 2.0, 1.0, 2.0),
            ReputationSignals(0.5, 0.5, 0.5, 0.5, 0.5, 0.5),
            0.5,
            0.5,
        ),
    )


def request(size: int = 2) -> RankingRequest:
    return RankingRequest(
        tuple(candidate(i) for i in range(size)),
        LensStack("test", (LensWeight(BuiltInLens.FOLLOWING, 1.0),)),
        DiversityPolicy(1.0, ()),
        200,
    )


def frame(value: Json) -> bytes:
    return json.dumps(value, separators=(",", ":"), allow_nan=True).encode() + b"\n"


def payload(req: RankingRequest) -> dict[str, Json]:
    return object_value(decode(json.dumps(asdict(req)).encode() + b"\n"))


def envelope(body: Json, identity: int = 1) -> dict[str, Json]:
    return {"protocol": PROTOCOL, "id": identity, "method": "rank", "request": body}


def exchange(data: bytes) -> list[dict[str, Json]]:
    result = subprocess.run(
        [sys.executable, "-m", "babble_algorithms.worker"],
        input=data,
        capture_output=True,
        timeout=15,
    )
    assert result.returncode == 0 and result.stderr == b"", result.stderr
    return [object_value(decode(line)) for line in result.stdout.splitlines(keepends=True)]


@pytest.mark.parametrize(
    "lens,expected",
    [
        (BuiltInLens.FOLLOWING, 0.775),
        (BuiltInLens.FRIENDS, 0.61),
        (BuiltInLens.RESEARCH, 0.5),
        (BuiltInLens.INTELLECTUAL_SERENDIPITY, 0.25 * (1.0 - 0.12 / 0.62) + 0.375),
        (BuiltInLens.CONTRADICTIONS, 0.54),
        (BuiltInLens.EMERGING, 0.45),
        (BuiltInLens.SLOW_INTERNET, 0.5),
        (BuiltInLens.WEIRD, 0.475),
    ],
)
def test_all_eight_lenses(lens: BuiltInLens, expected: float) -> None:
    req = replace(request(1), lens=LensStack("each", (LensWeight(lens, 1.0),)))
    result = rank(parse_ranking_request(payload(req)))
    part = result.trace.candidates[0].lens_contributions[0]
    assert part.lens_id == lens.id
    assert part.score == pytest.approx(expected)
    assert result.ranked[0].score == pytest.approx(expected)
    assert tuple(r.signal for r in result.ranked[0].reasons) == tuple(
        f"{lens.id}:{r.signal}" for r in part.reasons
    )


def test_source_membership_bonus_ignores_weight_and_primary() -> None:
    obj = candidate()
    obj = replace(
        obj,
        sources=(
            *obj.sources,
            CandidateSourceContribution("Emerging", 0.0),
            CandidateSourceContribution("Contradiction", 0.0),
        ),
    )
    reasons = {r.signal: r.contribution for r in lens_reasons(obj, BuiltInLens.WEIRD)}
    assert reasons["emerging"] == 0.1
    assert reasons["contradiction"] == pytest.approx(0.075)


def test_evidence_overflow_saturates_like_rust_and_weird_can_exceed_one() -> None:
    evidence = EvidenceSignals(1e308, 1e308, 1e308, 1e308)
    assert evidence.support_score() == evidence.contradiction_score() == 1.0
    obj = candidate(source="Emerging")
    obj = replace(
        obj,
        sources=(*obj.sources, CandidateSourceContribution("Contradiction")),
        signals=replace(
            obj.signals,
            relevance=0,
            novelty=1,
            exploration=1,
            contradiction=1,
            reputation=ReputationSignals(1, 1, 1, 1, 1, 1),
        ),
    )
    result = rank(
        replace(
            request(),
            candidates=(obj,),
            lens=LensStack("weird", (LensWeight(BuiltInLens.WEIRD, 1),)),
        )
    )
    assert result.trace.candidates[0].score == pytest.approx(1.1)
    assert result.ranked[0].score == 1.0


@pytest.mark.parametrize("weights", [(), (LensWeight(BuiltInLens.WEIRD, 0),)])
def test_empty_zero_fallback(weights: tuple[LensWeight, ...]) -> None:
    expected = rank(request())
    assert rank(replace(request(), lens=LensStack("test", weights))) == expected


def test_huge_blend_normalization_and_weighted_reasons() -> None:
    weights = tuple(LensWeight(lens, 1e308) for lens in BuiltInLens)
    result = rank(replace(request(), lens=LensStack("huge", weights)))
    for item in result.trace.candidates:
        assert len(item.lens_contributions) == 8
        assert all(part.weight == 0.125 for part in item.lens_contributions)
        assert item.score == pytest.approx(sum(part.score for part in item.lens_contributions))
        for part in item.lens_contributions:
            assert part.score == pytest.approx(sum(reason.contribution for reason in part.reasons))
    assert all(math.isfinite(item.score) for item in result.ranked)


def test_nanosecond_and_equivalent_offset_ties_sort_ids_ascending() -> None:
    base = candidate()
    objects = (
        replace(base, object_id=f"obj_{3:064x}", created_at="2026-09-30T00:00:00.000000001Z"),
        replace(base, object_id=f"obj_{2:064x}", created_at="2026-09-30T00:00:00.000000002Z"),
        replace(base, object_id=f"obj_{1:064x}", created_at="2026-09-30T01:00:00.000000002+01:00"),
    )
    result = rank(replace(request(), candidates=objects))
    assert [entry.object_id for entry in result.trace.candidates] == [
        f"obj_{i:064x}" for i in (1, 2, 3)
    ]
    assert [entry.candidate.object_id for entry in result.ranked] == [
        f"obj_{i:064x}" for i in (1, 2, 3)
    ]


def test_soft_floors_share_and_filtered_order() -> None:
    req = replace(
        request(4),
        candidates=(
            candidate(0),
            candidate(1),
            candidate(2, "Exploration"),
            candidate(3, "Evidence"),
        ),
        limit=2,
        diversity=DiversityPolicy(
            0.55, (SourceFloor("Exploration", 1), SourceFloor("Evidence", 0))
        ),
    )
    result = rank(req)
    assert [entry.candidate.object_id for entry in result.ranked] == [
        candidate(0).object_id,
        candidate(2).object_id,
    ]
    assert result.diversity_trace.candidates[0].reasons == ()
    assert result.diversity_trace.candidates[1].reasons[0].contribution == 0.18
    assert result.diversity_trace.filtered == (candidate(1).object_id, candidate(3).object_id)
    assert result.diversity_trace.policy.source_floors == (SourceFloor("Exploration", 1),)
    # A floor is a bonus, not a mandatory reservation.
    low = replace(
        candidate(9, "Exploration"),
        signals=replace(
            candidate().signals,
            followed_author=False,
            temporal=0,
            relevance=0,
            reputation=ReputationSignals(),
        ),
    )
    result = rank(replace(req, candidates=(candidate(0), candidate(1), low)))
    assert low.object_id in result.diversity_trace.filtered


def test_diversity_floors_are_prepared_once_without_changing_selection(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from babble_algorithms import ranking_diversity

    calls = 0
    original = ranking_diversity._prepare_policy  # pyright: ignore[reportPrivateUsage]

    def counted(policy: DiversityPolicy) -> object:
        nonlocal calls
        calls += 1
        return original(policy)

    req = replace(
        request(4),
        candidates=(
            candidate(0),
            candidate(1),
            candidate(2, "Exploration"),
            candidate(3, "Evidence"),
        ),
        limit=2,
        diversity=DiversityPolicy(
            0.55, (SourceFloor("Exploration", 1), SourceFloor("Evidence", 0))
        ),
    )
    expected = rank(req)
    monkeypatch.setattr(ranking_diversity, "_prepare_policy", counted)
    actual = rank(req)
    assert actual == expected
    assert calls == 1


def test_deterministic_permutation_property() -> None:
    rng = random.Random(291)
    sources: tuple[CandidateSource, ...] = ("Following", "Evidence", "Exploration", "Contradiction")
    objects = [
        replace(
            candidate(i, rng.choice(sources)),
            signals=replace(
                candidate().signals,
                relevance=rng.random(),
                novelty=rng.random(),
                temporal=rng.random(),
            ),
        )
        for i in range(30)
    ]
    req = replace(
        request(),
        candidates=tuple(objects),
        limit=15,
        lens=LensStack("mixed", tuple(LensWeight(lens, rng.random()) for lens in BuiltInLens)),
        diversity=DiversityPolicy(0.3, (SourceFloor("Exploration", 3),)),
    )
    expected = rank(req)
    for _ in range(25):
        rng.shuffle(objects)
        actual = rank(replace(req, candidates=tuple(objects)))
        assert actual == expected
        assert len({item.candidate.object_id for item in actual.ranked}) == 15
        assert all(0 <= item.score <= 1 for item in actual.ranked)


def test_unknown_or_missing_fields_at_every_record() -> None:
    def records(value: Json) -> list[dict[str, Json]]:
        if isinstance(value, dict):
            return [value, *(child for item in value.values() for child in records(item))]
        if isinstance(value, list):
            return [child for item in value for child in records(item)]
        return []

    req = replace(request(1), diversity=DiversityPolicy(0.5, (SourceFloor("Exploration", 1),)))
    for index in range(len(records(payload(req)))):
        for missing in (True, False):
            body = payload(req)
            target = records(body)[index]
            if missing:
                _ = target.pop(next(iter(target)))
            else:
                target["private_history"] = "SECRET"
            with pytest.raises(InvalidRequest):
                _ = parse_ranking_request(body)


@pytest.mark.parametrize("bad", [True, False, None, "0.5", -1, 1.01, math.nan, math.inf])
def test_numeric_unit_signals_reject_bad_values(bad: Json) -> None:
    body = payload(request(1))
    candidates = body["candidates"]
    assert isinstance(candidates, list)
    signals = object_value(object_value(candidates[0])["signals"])
    for key in (
        "social_distance",
        "relevance",
        "novelty",
        "evidence_quality",
        "contradiction",
        "temporal",
        "exploration",
    ):
        original = signals[key]
        signals[key] = bad
        response = handle(frame(envelope(body)), AlgorithmExecutor())
        assert response.error is not None and response.error.code == "invalid_request"
        signals[key] = original


def test_duplicate_keys_nonfinite_and_bad_frame_recover() -> None:
    valid = frame(envelope(payload(request())))
    bad = [
        valid.replace(b'"limit":200', b'"limit":200,"limit":1'),
        valid.replace(b'"weight":1.0', b'"weight":NaN', 1),
        valid.replace(b'"weight":1.0', b'"weight":1e999', 1),
        valid.replace(b'"weight":1.0', b'"weight":' + b"9" * 400, 1),
    ]
    responses = exchange(b"".join(bad) + valid)
    assert all(response["error"] is not None for response in responses[:-1])
    assert responses[-1]["error"] is None


@pytest.mark.parametrize(
    "field,bad",
    [
        ("limit", 0),
        ("limit", 201),
        ("limit", True),
        ("limit", 1.5),
        ("candidates", [None] * 201),
        ("lens", {"id": "a b", "weights": []}),
        ("lens", {"id": "", "weights": []}),
        ("lens", {"id": "x" * 129, "weights": []}),
        ("lens", {"id": "ok", "weights": [{"lens": "following", "weight": 1}]}),
        ("lens", {"id": "ok", "weights": [{"lens": "Following", "weight": True}]}),
        ("lens", {"id": "ok", "weights": [{"lens": "Following", "weight": 1}] * 2}),
        ("diversity", {"max_source_share": True, "source_floors": []}),
        ("diversity", {"max_source_share": -0.1, "source_floors": []}),
        ("diversity", {"max_source_share": 1.1, "source_floors": []}),
        (
            "diversity",
            {"max_source_share": 1, "source_floors": [{"source": "Evidence", "minimum": 1}] * 2},
        ),
        (
            "diversity",
            {"max_source_share": 1, "source_floors": [{"source": "Evidence", "minimum": True}]},
        ),
        (
            "diversity",
            {"max_source_share": 1, "source_floors": [{"source": "Evidence", "minimum": 201}]},
        ),
    ],
)
def test_request_limits(field: str, bad: Json) -> None:
    body = payload(request())
    body[field] = bad
    with pytest.raises(InvalidRequest):
        _ = parse_ranking_request(body)


@pytest.mark.parametrize(
    "field,bad",
    [
        ("object_id", "obj_short"),
        ("object_id", "obj_" + "g" * 64),
        ("source", "following"),
        ("sources", []),
        ("sources", [{"source": "Evidence", "weight": 1}]),
        ("sources", [{"source": "Following", "weight": 1}] * 2),
        ("sources", [{"source": "Following", "weight": 1.1}]),
        ("created_at", "2026-02-30T00:00:00Z"),
        ("created_at", "2026-09-30"),
        ("created_at", "2026-09-30T00:00:00+24:00"),
    ],
)
def test_candidate_limits(field: str, bad: Json) -> None:
    body = payload(request(1))
    objects = body["candidates"]
    assert isinstance(objects, list)
    object_value(objects[0])[field] = bad
    with pytest.raises(InvalidRequest):
        _ = parse_ranking_request(body)
    body = payload(replace(request(), candidates=(candidate(), candidate())))
    with pytest.raises(InvalidRequest):
        _ = parse_ranking_request(body)


def test_maximum_request_full_trace_and_empty_candidates() -> None:
    req = replace(
        request(200), lens=LensStack("full", tuple(LensWeight(lens, 1.0) for lens in BuiltInLens))
    )
    health: Json = {"protocol": PROTOCOL, "id": 2, "method": "health"}
    responses = exchange(
        frame(health) + frame(envelope(payload(req))) + frame(envelope(payload(request(0)), 3))
    )
    assert all(response["error"] is None for response in responses)
    assert object_value(responses[0]["result"])["ranking_provider"] == asdict(RANKING_PROVIDER)
    result = object_value(responses[1]["result"])
    assert set(result) == {"ranked", "trace", "diversity_trace", "provider"}
    assert result["provider"] == asdict(RANKING_PROVIDER)
    ranked = result["ranked"]
    assert isinstance(ranked, list) and len(ranked) == 200
    assert 1024 * 1024 < len(frame(result)) < MAX_LINE_BYTES
    assert object_value(responses[2]["result"])["ranked"] == []


def test_persistent_worker_flushes_rank_before_eof_with_deadline() -> None:
    with subprocess.Popen(
        [sys.executable, "-m", "babble_algorithms.worker"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
    ) as process:
        try:
            assert process.stdin is not None and process.stdout is not None
            for identity in range(1, 4):
                _ = process.stdin.write(frame(envelope(payload(request()), identity)))
                process.stdin.flush()
                ready, _, _ = select.select([process.stdout], [], [], 5)
                assert ready, "ranking worker missed response deadline"
                response = object_value(decode(cast(bytes, process.stdout.readline())))
                assert response["id"] == identity and response["error"] is None
            process.stdin.close()
            assert process.wait(timeout=5) == 0
        finally:
            if process.poll() is None:
                process.kill()
                _ = process.wait(timeout=5)


def test_judgment_frame_limit_retained_after_ranking_expansion() -> None:
    judgment: Json = {
        "protocol": PROTOCOL,
        "id": 1,
        "method": "judge",
        "request": {
            "definition": "babble.judgment.spam.v1",
            "state": {"subject": "obj_test", "context": {"text": "hello"}},
            "parameters": {},
        },
    }
    base = frame(judgment).rstrip(b"\n")
    exact = base + b" " * (MAX_JUDGMENT_LINE_BYTES - len(base) - 1) + b"\n"
    responses = exchange(exact + exact[:-1] + b" \n")
    assert responses[0]["error"] is None
    assert responses[1]["error"] is not None
    oversized_unknown = exact[:-1].replace(
        b"babble.judgment.spam.v1", b"babble.judgment.unknown.v1"
    )
    rejected = handle(oversized_unknown + b"\n", AlgorithmExecutor())
    assert rejected.error is not None and rejected.error.code == "invalid_request"


def test_rust_golden_ranking_parity() -> None:
    path = Path(__file__).parents[2] / "fixtures/protocol/v1/ranking.json"
    fixtures = object_value(decode(path.read_bytes().rstrip() + b"\n"))
    cases = fixtures["cases"]
    assert isinstance(cases, list) and len(cases) >= 8
    for raw in cases:
        case = object_value(raw)
        expected = object_value(case["result"])
        expected["provider"] = {"provider": "babble-python", "model": "lenses-v1", "version": "1"}
        response = handle(frame(envelope(case["request"])), AlgorithmExecutor())
        assert response.error is None, case.get("name")
        actual = object_value(decode(encode(response)))["result"]
        assert_json_close(actual, expected)


def test_generated_wire_schema_limits_match_python() -> None:
    path = Path(__file__).parents[2] / "fixtures/algorithms/v1/schema-bundle.json"
    schema = object_value(decode(path.read_bytes().rstrip() + b"\n"))
    limits = object_value(schema["limits"])
    assert limits["line_bytes"] == MAX_LINE_BYTES
    assert limits["json_nodes"] == MAX_NODES
    assert limits["json_depth"] == MAX_DEPTH
    assert limits["array_items"] == MAX_ARRAY_ITEMS
    assert limits["judgment_line_bytes"] == MAX_JUDGMENT_LINE_BYTES
    assert limits["judgment_json_nodes"] == MAX_JUDGMENT_NODES
    assert schema["ranking_provider"] == asdict(RANKING_PROVIDER)


def test_timestamp_integer_precision() -> None:
    assert (
        timestamp_nanos("2026-09-30T00:00:00.000000002Z")
        - timestamp_nanos("2026-09-30T00:00:00.000000001Z")
        == 1
    )
    assert timestamp_nanos("0000-02-29T00:00:00Z") < timestamp_nanos("0001-01-01T00:00:00Z")


@pytest.mark.parametrize(
    "raw,canonical",
    [
        ("2026-09-30t00:00:00.0100000000z", "2026-09-30T00:00:00.01Z"),
        ("2026-09-30 00:00:00-00:00", "2026-09-30T00:00:00Z"),
        ("2026-09-30X00:00:00+00:00", "2026-09-30T00:00:00Z"),
        ("2016-12-31T23:59:60Z", "2016-12-31T23:59:59.999999999Z"),
        ("2017-01-01T00:59:60+01:00", "2017-01-01T00:59:59.999999999+01:00"),
        ("2026-09-30T00:00:00.123456789999+01:30", "2026-09-30T00:00:00.123456789+01:30"),
    ],
)
def test_timestamp_canonicalization_matches_rust(raw: str, canonical: str) -> None:
    assert parse_timestamp(raw)[0] == canonical
    req = replace(request(1), candidates=(replace(candidate(), created_at=raw),))
    assert parse_ranking_request(payload(req)).candidates[0].created_at == canonical


@pytest.mark.parametrize(
    "raw",
    [
        "2016-12-30T23:59:60Z",
        "2016-12-31T22:59:60Z",
        "2016-12-31T23:59:60+01:00",
        "2026-09-30T24:00:00Z",
        "2026-09-30T00:60:00Z",
        "2026-09-30T00:00:61Z",
        "2026-09-30T00:00:00+00:60",
        "2026-09-30T00:00:00",
        "2026-09-30TT00:00:00Z",
    ],
)
def test_invalid_timestamp_edges(raw: str) -> None:
    with pytest.raises(ValueError):
        _ = parse_timestamp(raw)


def test_numeric_boolean_mutation_fuzz_covers_every_numeric_field() -> None:
    def numeric_fields(value: Json) -> list[tuple[dict[str, Json], str]]:
        fields: list[tuple[dict[str, Json], str]] = []
        if isinstance(value, dict):
            for key, child in value.items():
                if isinstance(child, (int, float)) and not isinstance(child, bool):
                    fields.append((value, key))
                fields.extend(numeric_fields(child))
        elif isinstance(value, list):
            for child in value:
                fields.extend(numeric_fields(child))
        return fields

    req = replace(request(1), diversity=DiversityPolicy(0.5, (SourceFloor("Evidence", 1),)))
    body = payload(req)
    fields = numeric_fields(body)
    rng = random.Random(2901)
    for _ in range(200):
        target, key = rng.choice(fields)
        original = target[key]
        target[key] = rng.choice((True, False, None, "1", -1, math.nan, math.inf, -math.inf))
        response = handle(frame(envelope(body)), AlgorithmExecutor())
        assert response.error is not None and response.error.code == "invalid_request"
        target[key] = original


def test_ranking_failure_and_diagnostics_do_not_pollute_wire(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def fail(_self: AlgorithmExecutor, _request: RankingRequest) -> None:
        print("PRIVATE_DIAGNOSTIC")
        raise RuntimeError("PRIVATE_EXCEPTION")

    monkeypatch.setattr(AlgorithmExecutor, "rank", fail)
    response = handle(frame(envelope(payload(request()))), AlgorithmExecutor())
    assert response.error is not None and response.error.code == "algorithm_failure"
    assert b"PRIVATE" not in encode(response)
