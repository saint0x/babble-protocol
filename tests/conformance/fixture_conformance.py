from __future__ import annotations

import argparse
import json
import math
import struct
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

PREAMBLE = b"babble.canonical.v1\0"
MAX_I64 = 9_223_372_036_854_775_807
MIN_I64 = -9_223_372_036_854_775_808
MAX_U64 = 18_446_744_073_709_551_615


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Independent stdlib conformance checks for Babble Protocol fixtures."
    )
    parser.add_argument("fixtures", type=Path)
    parser.add_argument("schema_bundle", type=Path)
    args = parser.parse_args()

    fixtures = json.loads(args.fixtures.read_text())
    bundle = json.loads(args.schema_bundle.read_text())

    require(bundle["protocol"] == "babble", "schema bundle protocol must be babble")
    require(bundle["generated_by"] == "babble-schema", "schema bundle generator must be babble-schema")
    require(bundle["fixtures"] == fixtures, "schema bundle embedded fixtures must match fixtures.json")

    verify_canonical(fixtures["canonical_encoding"])
    inventory = fixtures["bundle_manifest"]
    require(inventory["version"] == "babble.canonical.v1", "bundle canonical version mismatch")
    require(
        canonical_value_bytes(inventory["sample"]).hex() == inventory["bytes_hex"],
        "independent bundle canonical bytes mismatch",
    )
    require(valid_hex_string(inventory["hash"], 64), "bundle hash must be 32-byte hex")
    verify_schemas(bundle["schemas"])
    verify_registries(fixtures)
    verify_rpc(fixtures["rpc_catalog"], fixtures["rpc_request"], fixtures["rpc_error_response"])
    verify_media(fixtures["media_blob"], fixtures["api_publish_media_object_request"])

    print(
        json.dumps(
            {
                "ok": True,
                "checked": "Babble Protocol fixture conformance",
                "implementation": "stdlib-python-independent",
            },
            separators=(",", ":"),
        )
    )


def verify_canonical(fixture: Mapping[str, Any]) -> None:
    require(fixture["version"] == "babble.canonical.v1", "canonical fixture version mismatch")
    sample = {
        "zeta": [1, "1", 1.0],
        "alpha": {"nested": True, "empty": None},
    }
    require(sample == fixture["sample"], "canonical fixture sample drifted")
    encoded = canonical_value_bytes(sample)
    require(encoded.hex() == fixture["bytes_hex"], "independent canonical encoder bytes mismatch")
    require(valid_hex_string(fixture["hash"], 64), "canonical hash must be 32-byte hex")


def verify_schemas(schemas: Mapping[str, Any]) -> None:
    for name in [
        "object.Object",
        "object.BundleManifest",
        "graph.Edge",
        "state.Event",
        "judgment.JudgmentRegistry",
        "rpc.RpcCatalog",
        "runtime.SurfaceSessionPlan",
        "realtime.RoomSpec",
        "personalization.LocalUserModel",
        "personalization.EncryptedLocalUserModel",
        "personalization.PersonalizationObjectSummary",
        "personalization.PersonalizationSyncRecipient",
        "personalization.PersonalizationTrace",
        "api.PrepareSurfaceResponse",
        "api.BrowserInvocationResponse",
        "api.BrowserInvocationResult",
        "api.AcknowledgeBrowserInvocationRequest",
        "api.BrowserExecutionTicket",
    ]:
        require(name in schemas, f"schema bundle missing {name}")


def verify_registries(fixtures: Mapping[str, Any]) -> None:
    schema_registry = expect_object(fixtures["schema_registry"], "schema_registry")
    protocol = expect_object(schema_registry["protocol"], "schema_registry.protocol")
    require(protocol["name"] == "babble", "schema registry protocol mismatch")
    capabilities = set(expect_array(schema_registry["core_capabilities"], "core_capabilities"))
    for capability in [
        "babble.realtime.join",
        "babble.realtime.send",
        "babble.graphics.webgpu",
        "babble.network.fetch",
    ]:
        require(capability in capabilities, f"missing core capability {capability}")

    definitions = expect_array(
        expect_object(fixtures["judgment_registry"], "judgment_registry")["definitions"],
        "judgment definitions",
    )
    outputs = {definition["id"]: definition["output_schema"] for definition in definitions}
    for judgment_id, schema in {
        "babble.judgment.spam.v1": "babble.judgment.output.probability.v1",
        "babble.judgment.evidence_quality.v1": "babble.judgment.output.bounded_score.v1",
        "babble.judgment.content_analysis.v1": "babble.judgment.output.content_analysis.v1",
        "babble.judgment.moderation.v1": "babble.judgment.output.moderation.v1",
    }.items():
        require(outputs.get(judgment_id) == schema, f"judgment definition/output mismatch for {judgment_id}")

    realtime_schemas = set(
        expect_array(
            expect_object(fixtures["realtime_schema_registry"], "realtime_schema_registry")["schemas"],
            "realtime schemas",
        )
    )
    require("babble.realtime.state.v1" in realtime_schemas, "missing realtime state schema")
    require("babble.realtime.chat.v1" in realtime_schemas, "missing realtime chat schema")


def verify_rpc(catalog: Mapping[str, Any], request: Mapping[str, Any], response: Mapping[str, Any]) -> None:
    require(catalog["protocol"] == "babble.rpc.v1", "RPC catalog protocol mismatch")
    methods: dict[str, Mapping[str, Any]] = {}
    for method in expect_array(catalog["methods"], "rpc methods"):
        method = expect_object(method, "rpc method")
        name = expect_string(method["method"], "rpc method")
        version = method["version"]
        require(type(version) is int and version >= 1, "RPC method version must be a positive integer")
        require(name.startswith("babble."), f"invalid RPC method namespace {name}")
        suffix = name.rsplit(".", 1)[-1]
        if suffix.startswith("v") and suffix[1:].isdigit():
            require(name.endswith(f".v{version}"), f"invalid RPC method namespace/version {name}")
        else:
            require(version == 2, f"unversioned RPC method must be version 2: {name}")
        require(expect_string(method["input"], "rpc input"), "RPC input must be present")
        require(expect_string(method["output"], "rpc output"), "RPC output must be present")
        require(int(method["timeout_ms"]) > 0, "RPC timeout must be positive")
        require(
            method["idempotency"]
            in {"read_only", "idempotent_by_input", "requires_idempotency_key", "non_idempotent"},
            f"unsupported RPC idempotency mode {method['idempotency']}",
        )
        require(name not in methods, f"duplicate RPC method {name}")
        methods[name] = method

    for required in [
        "babble.object.publish_text.v1",
        "babble.object.fork.v1",
        "babble.object.remix.v1",
        "babble.graph.relationship.infer.v1",
        "babble.graph.evidence.v1",
        "babble.graph.traverse.v1",
        "babble.runtime.surface.prepare.v1",
        "babble.runtime.surface.health.v1",
        "babble.runtime.surface.session.schedule.v1",
        "babble.runtime.surface.session.apply_schedule.v1",
        "babble.runtime.surface.session.state.checkpoint.v1",
        "babble.runtime.surface.session.state.get.v1",
        "babble.observability.snapshot.v1",
        "babble.realtime.room.define.v1",
        "babble.realtime.message.publish.v1",
        "babble.judgment.definitions.list.v1",
        "babble.judgment.providers.list.v1",
        "babble.lenses.list.v1",
        "babble.capabilities.list.v1",
    ]:
        require(required in methods, f"missing RPC method {required}")

    for action in ("follow", "unfollow", "share", "reply"):
        name = f"babble.social.{action}"
        require(name in methods, f"missing one-use social method {name}")
        method = methods[name]
        require(method["output"] == "api.InvocationSocialResult", f"invalid invocation output for {name}")
        require(method["idempotency"] == "requires_idempotency_key", f"missing stable invocation key for {name}")
        require(method["capability"] == {"capability": f"babble.social.{action}", "version": 1, "required": True},
                f"invalid social capability declaration for {name}")

    for action in ("clipboard.write", "fullscreen.enter"):
        name = f"babble.{action}"
        require(f"babble.{action}.v1" in methods, f"missing retained v1 registry entry for {action}")
        require(name in methods, f"missing durable browser method {name}")
        method = methods[name]
        require(method["output"] == "api.BrowserInvocationResult", f"invalid browser result for {name}")
        require(method["idempotency"] == "requires_idempotency_key", f"missing stable browser invocation key for {name}")
        require(method["capability"] == {"capability": f"babble.{action}", "version": 1, "required": True},
                f"invalid browser capability declaration for {name}")

    require(request["protocol"] == "babble.rpc.v1", "RPC request protocol mismatch")
    require(request["method"] in methods, "RPC request method missing from catalog")
    require(request["id"], "RPC request id must be non-empty")
    require(
        request["payload"]["object_id"]
        == "obj_0000000000000000000000000000000000000000000000000000000000000000",
        "RPC request payload object id mismatch",
    )
    binding = expect_object(request["binding"], "rpc binding")
    require(binding["runtime_id"] == "fixture-runtime", "RPC binding runtime mismatch")
    require(binding["origin"] == "babble://fixture", "RPC binding origin mismatch")
    require(int(request["deadline"]["timeout_ms"]) == 30000, "RPC request deadline mismatch")

    require(response["protocol"] == request["protocol"], "RPC response protocol mismatch")
    require(response["id"] == request["id"], "RPC response id mismatch")
    require(response["result"] is None, "RPC error response result must be null")
    error = expect_object(response["error"], "rpc error")
    require(error["code"] == "NOT_FOUND", "RPC error code mismatch")
    require(error["retryable"] is False, "RPC not-found error must not be retryable")
    require(response["trace_id"] == request["trace_id"], "RPC trace id mismatch")


def verify_media(blob: Mapping[str, Any], request: Mapping[str, Any]) -> None:
    integrity = expect_string(blob["integrity"], "media integrity")
    require(valid_hex_string(integrity, 64), "media integrity must be 32-byte hex")
    require(blob["uri"] == f"babble://blobs/{integrity}", "media blob URI must contain integrity hash")
    require(blob["media_type"] == "text/plain", "media type mismatch")
    require(int(blob["size_bytes"]) == 14, "media size mismatch")
    require(valid_prefixed_id(request["author_id"], "id_"), "publish media author id malformed")
    resources = expect_array(request["resources"], "publish media resources")
    require(len(resources) == 1, "publish media fixture must contain one resource")
    require(resources[0] == blob, "publish media resource must match blob fixture")


def canonical_value_bytes(value: Any) -> bytes:
    out = bytearray(PREAMBLE)
    encode_canonical(value, out)
    return bytes(out)


def encode_canonical(value: Any, out: bytearray) -> None:
    if value is None:
        out.extend(b"n")
    elif isinstance(value, bool):
        out.extend(b"t" if value else b"f")
    elif isinstance(value, int):
        encode_int(value, out)
    elif isinstance(value, float):
        encode_float(value, out)
    elif isinstance(value, str):
        encode_string(value, out)
    elif isinstance(value, Mapping):
        out.extend(b"o")
        encode_u64(len(value), out)
        for key in sorted(value.keys(), key=lambda item: item.encode("utf-8")):
            encode_string(key, out)
            encode_canonical(value[key], out)
    elif isinstance(value, Sequence):
        out.extend(b"a")
        encode_u64(len(value), out)
        for item in value:
            encode_canonical(item, out)
    else:
        raise TypeError(f"unsupported canonical type: {type(value)!r}")


def encode_int(value: int, out: bytearray) -> None:
    if MIN_I64 <= value <= MAX_I64:
        out.extend(b"i")
        out.extend(value.to_bytes(8, "big", signed=True))
    elif 0 <= value <= MAX_U64:
        out.extend(b"u")
        encode_u64(value, out)
    else:
        raise ValueError(f"canonical integer must fit in i64 or u64: {value}")


def encode_float(value: float, out: bytearray) -> None:
    require(math.isfinite(value), "canonical float must be finite")
    out.extend(b"d")
    out.extend(struct.pack(">d", value))


def encode_string(value: str, out: bytearray) -> None:
    encoded = value.encode("utf-8")
    out.extend(b"s")
    encode_u64(len(encoded), out)
    out.extend(encoded)


def encode_u64(value: int, out: bytearray) -> None:
    require(0 <= value <= MAX_U64, "canonical length/integer must fit in u64")
    out.extend(value.to_bytes(8, "big", signed=False))


def expect_object(value: Any, label: str) -> Mapping[str, Any]:
    require(isinstance(value, Mapping), f"{label} must be an object")
    return value


def expect_array(value: Any, label: str) -> Sequence[Any]:
    require(isinstance(value, list), f"{label} must be an array")
    return value


def expect_string(value: Any, label: str) -> str:
    require(isinstance(value, str), f"{label} must be a string")
    return value


def valid_prefixed_id(value: str, prefix: str) -> bool:
    return value.startswith(prefix) and valid_hex_string(value.removeprefix(prefix), 64)


def valid_hex_string(value: str, length: int) -> bool:
    if not isinstance(value, str) or len(value) != length:
        return False
    try:
        bytes.fromhex(value)
    except ValueError:
        return False
    return True


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


if __name__ == "__main__":
    main()
