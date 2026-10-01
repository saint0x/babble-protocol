"""Fault-injection peer for Rust transport tests, not an algorithm implementation."""
import json
import os
import sys
import time

MODE = sys.argv[1]
PROVIDER = {"provider": "babel-python", "model": "lexical-v1", "version": "1"}
RANKING_PROVIDER = {"provider": "babel-python", "model": "lenses-v1", "version": "1"}
TEMPORAL_PROVIDER = {"provider": "babel-python", "model": "temporal-v1", "version": "1"}
DEFINITIONS = ["babel.judgment." + name + ".v1" for name in (
    "spam", "relevance", "relationship", "evidence_quality", "content_analysis", "moderation",
    "source_agreement"
)]

for line in sys.stdin:
    request = json.loads(line)
    health = request["method"] == "health"
    result = {"provider": PROVIDER, "supported_definitions": DEFINITIONS, "ranking_provider": RANKING_PROVIDER, "temporal_provider": TEMPORAL_PROVIDER} if health else {
        "provider": PROVIDER,
        "output": {"kind": "probability", "score": 0.25, "confidence": 0.75,
                   "label": "test", "pid": os.getpid()},
        "confidence": 0.75,
    }
    response = {"protocol": "babel.algorithms.v1", "id": request["id"], "result": result, "error": None}
    if health:
        if MODE == "health_timeout":
            time.sleep(30)
        if MODE == "health_provider":
            result["provider"] = {**PROVIDER, "version": "999"}
        if MODE == "health_definitions":
            result["supported_definitions"] = DEFINITIONS[:-1]
        if MODE == "health_ranking":
            result["ranking_provider"] = {**RANKING_PROVIDER, "version": "999"}
    else:
        if MODE == "confidence_mismatch":
            result["output"]["confidence"] = 0.6
        if MODE == "environment":
            result["output"]["environment_keys"] = sorted(os.environ)
        if MODE == "timeout":
            time.sleep(30)
        if MODE == "truncated":
            sys.stdout.write('{"protocol":')
            sys.stdout.flush()
            sys.exit(0)
        if MODE == "oversize":
            sys.stdout.write("x" * (4 * 1024 * 1024 + 1))
            sys.stdout.flush()
            time.sleep(30)
        if MODE == "malformed":
            print("not JSON", flush=True)
            continue
        if MODE == "invalid_utf8":
            sys.stdout.buffer.write(b"\xff\n")
            sys.stdout.flush()
            continue
        if MODE == "exit":
            sys.stderr.write("PRIVATE-CONTENT traceback secret\n")
            sys.exit(17)
        if MODE == "restart" and request["id"] == 2:
            sys.exit(17)
        if MODE == "stale_id":
            response["id"] -= 1
        if MODE == "protocol":
            response["protocol"] = "babel.algorithms.v0"
        if MODE == "provider":
            result["provider"] = {**PROVIDER, "model": "pretend"}
        if MODE == "shape":
            result["output"]["kind"] = "bounded_score"
        if MODE == "score":
            result["output"]["score"] = 1.01
        if MODE == "confidence":
            result["confidence"] = -1
        if MODE == "nan":
            result["confidence"] = float("nan")
        if MODE == "nested_nan":
            result["output"]["extra"] = {"value": float("nan")}
        if MODE == "judgment_nodes":
            result["output"]["extra"] = [[0] * 256 for _ in range(17)]
        if MODE == "error":
            response["result"] = None
            response["error"] = {"code": "algorithm_failure", "message": "PRIVATE-CONTENT traceback secret"}
        if MODE == "missing_null":
            del response["error"]
        if MODE == "both":
            response["error"] = {"code": "algorithm_failure", "message": "no"}
        if MODE == "duplicate":
            print(json.dumps(response).replace('"score": 0.25', '"score": 0.25, "score": 0.5'), flush=True)
            continue
        if MODE == "multiple":
            sys.stdout.write(json.dumps(response) + "\n" + json.dumps(response) + "\n")
            sys.stdout.flush()
            continue
    print(json.dumps(response), flush=True)
    if health and MODE == "blocked_write":
        time.sleep(30)
