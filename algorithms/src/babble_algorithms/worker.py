"""Run with python -m babble_algorithms.worker; stdout is exclusively NDJSON."""

from __future__ import annotations

import json
import sys
from contextlib import redirect_stdout
from dataclasses import asdict, dataclass
from typing import BinaryIO, cast

from babble_algorithms.execution import AlgorithmExecutor, HealthResult, JudgeResult
from babble_algorithms.ranking_types import RankingResult
from babble_algorithms.temporal_types import TemporalResult
from babble_algorithms.wire import (
    MAX_ID,
    MAX_JUDGMENT_LINE_BYTES,
    MAX_JUDGMENT_NODES,
    MAX_LINE_BYTES,
    MAX_NODES,
    PROTOCOL,
    ErrorCode,
    HealthRequest,
    InvalidRequest,
    RankRequest,
    TemporalWorkerRequest,
    UnsupportedDefinition,
    decode,
    json_value,
    parse_request,
    request_id,
    validate_tree,
)


@dataclass(frozen=True, slots=True)
class Error:
    code: ErrorCode
    message: str

    def __post_init__(self) -> None:
        if self.code not in ("invalid_request", "unsupported_definition", "algorithm_failure"):
            raise ValueError("worker error code must be supported")
        if not isinstance(cast(object, self.message), str) or not self.message.strip():
            raise ValueError("worker error message must be non-empty")
        object.__setattr__(self, "message", " ".join(self.message.split()))


@dataclass(frozen=True, slots=True)
class Response:
    protocol: str
    id: int | None
    result: HealthResult | JudgeResult | RankingResult | TemporalResult | None
    error: Error | None

    def __post_init__(self) -> None:
        if self.protocol != PROTOCOL:
            raise ValueError("worker response protocol is unsupported")
        if self.id is not None and (type(self.id) is not int or not 1 <= self.id <= MAX_ID):
            raise ValueError("worker response id must be a positive safe integer or None")
        if self.result is None and self.error is None:
            raise ValueError("worker response must contain result or error")
        if self.result is not None and self.error is not None:
            raise ValueError("worker response cannot contain both result and error")
        if self.result is not None and not isinstance(
            cast(object, self.result), HealthResult | JudgeResult | RankingResult | TemporalResult
        ):
            raise ValueError("worker response result has an unsupported type")
        if self.error is not None and not isinstance(cast(object, self.error), Error):
            raise ValueError("worker response error must be Error")


def failure(identity: int | None, code: ErrorCode) -> Response:
    messages: dict[ErrorCode, str] = {
        "invalid_request": "Invalid algorithm worker request.",
        "unsupported_definition": "Unsupported Judgment definition.",
        "algorithm_failure": "Algorithm execution failed.",
    }
    return Response(PROTOCOL, identity, None, Error(code, messages[code]))


def handle(line: bytes, executor: AlgorithmExecutor) -> Response:
    identity: int | None = None
    try:
        value = decode(line)
        identity = request_id(value)
        if (
            isinstance(value, dict)
            and value.get("method") == "judge"
            and len(line) > MAX_JUDGMENT_LINE_BYTES
        ):
            raise InvalidRequest("Judgment frame exceeds the limit")
        request = parse_request(value)
    except UnsupportedDefinition:
        return failure(identity, "unsupported_definition")
    except InvalidRequest:
        return failure(identity, "invalid_request")
    try:
        # Protect the protocol even if a library starts writing diagnostics.
        with redirect_stdout(sys.stderr):
            if isinstance(request, HealthRequest):
                result = executor.health()
            elif isinstance(request, RankRequest):
                result = executor.rank(request.request)
            elif isinstance(request, TemporalWorkerRequest):
                result = executor.temporal(request.request)
            else:
                result = executor.judge(request.request)
        return Response(PROTOCOL, identity, result, None)
    except Exception:
        # Exception messages and tracebacks may contain private request content.
        return failure(identity, "algorithm_failure")


def encode(response: Response) -> bytes:
    try:
        judgment = isinstance(response.result, JudgeResult)
        limit = MAX_JUDGMENT_LINE_BYTES if judgment else MAX_LINE_BYTES
        value = json_value(asdict(response))
        # Output strings are bounded by the frame; summaries may exceed input text limits.
        validate_tree(value, MAX_JUDGMENT_NODES if judgment else MAX_NODES, max_text_bytes=limit)
        data = (
            json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode(
                "utf-8"
            )
            + b"\n"
        )
        if len(data) <= limit:
            return data
    except Exception:
        # Do not recurse through encode() here: the serializer itself may be the failing path.
        pass
    return _fallback_failure_frame(response.id)


def _fallback_failure_frame(identity: object) -> bytes:
    safe_id = identity if type(identity) is int and 1 <= identity <= MAX_ID else None
    id_json = b"null" if safe_id is None else str(safe_id).encode("ascii")
    return (
        b'{"protocol":"babble.algorithms.v1","id":'
        + id_json
        + b',"result":null,"error":{"code":"algorithm_failure",'
        + b'"message":"Algorithm execution failed."}}\n'
    )


def serve(source: BinaryIO, sink: BinaryIO) -> None:
    executor = AlgorithmExecutor()
    while True:
        line = source.readline(MAX_LINE_BYTES + 1)
        if not line:
            return
        if len(line) > MAX_LINE_BYTES or not line.endswith(b"\n"):
            # Stop rather than drain an attacker-controlled unbounded stream.
            _ = sink.write(encode(failure(None, "invalid_request")))
            sink.flush()
            return
        _ = sink.write(encode(handle(line, executor)))
        sink.flush()


def main() -> None:
    try:
        serve(sys.stdin.buffer, sys.stdout.buffer)
    except BrokenPipeError:
        # Avoid a second flush error during interpreter shutdown.
        sys.stdout = sys.stderr


if __name__ == "__main__":
    main()
