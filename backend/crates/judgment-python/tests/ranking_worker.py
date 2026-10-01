"""Mutate real worker responses to exercise the Rust ranking trust boundary."""
import json
import os
import sys
import time
from dataclasses import asdict

from babble_algorithms.execution import AlgorithmExecutor
from babble_algorithms.worker import handle

mode = sys.argv[1]
executor = AlgorithmExecutor()
rank_calls = 0
for line in sys.stdin.buffer:
    request = json.loads(line)
    response = asdict(handle(line, executor))
    if request["method"] == "rank":
        rank_calls += 1
        if mode == "timeout":
            time.sleep(30)
        if mode == "exit" or mode == "restart" and request["id"] == 2:
            sys.stderr.write("PRIVATE-CONTENT traceback\n")
            sys.exit(17)
        result = response["result"]
        if mode == "provider":
            result["provider"]["version"] = "999"
        elif mode == "extra":
            result["private_history"] = ["PRIVATE-CONTENT"]
        elif mode == "nested_extra":
            result["ranked"][0]["candidate"]["extra"] = True
        elif mode == "duplicate":
            result["ranked"].append(result["ranked"][0])
        elif mode == "mutation":
            result["ranked"][0]["candidate"]["signals"]["relevance"] = 0.987654321
        elif mode == "foreign":
            result["ranked"][0]["candidate"]["object_id"] = "obj_" + "f" * 64
        elif mode == "trace":
            result["trace"]["candidates"].pop()
        elif mode == "score":
            result["ranked"][0]["score"] = 1.25
        elif mode == "nan":
            result["trace"]["candidates"][0]["score"] = float("nan")
        elif mode == "filtered":
            result["diversity_trace"]["filtered"].append(result["ranked"][0]["candidate"]["object_id"])
        elif mode == "stale":
            response["id"] -= 1
        elif mode == "error":
            response["result"] = None
            response["error"] = {"code": "algorithm_failure", "message": "PRIVATE-CONTENT traceback"}
    elif request["method"] == "judge" and mode == "shared":
        response["result"]["output"]["rank_calls"] = rank_calls
        response["result"]["output"]["pid"] = os.getpid()
    print(json.dumps(response, separators=(",", ":")), flush=True)
