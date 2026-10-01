from __future__ import annotations

import json
from pathlib import Path
from typing import cast

import pytest

from babble_algorithms import (
    canonical_float,
    canonical_unsigned,
    canonical_value_bytes,
    canonical_value_hex,
)
from babble_algorithms.canonical import CanonicalValue
from babble_algorithms.wire import Json, object_value


def fixtures() -> dict[str, Json]:
    return object_value(cast(
        Json,
        json.loads(
            (Path(__file__).parents[2] / "fixtures/protocol/v1/fixtures.json").read_text()
        ),
    ))


def test_canonical_encoder_matches_rust_fixture_bytes() -> None:
    data = fixtures()
    sample = {
        "zeta": [1, "1", canonical_float(1.0)],
        "alpha": {
            "nested": True,
            "empty": None,
        },
    }

    canonical = object_value(data["canonical_encoding"])
    assert canonical["version"] == "babble.canonical.v1"
    assert canonical_value_hex(sample) == canonical["bytes_hex"]


def test_canonical_encoder_preserves_type_boundaries_and_key_order() -> None:
    assert canonical_value_hex("1") != canonical_value_hex(1)
    assert canonical_value_hex(1) != canonical_value_hex(canonical_float(1.0))
    assert canonical_value_hex({"beta": [2, 3], "alpha": {"z": True, "a": None}}) == (
        canonical_value_hex({"alpha": {"a": None, "z": True}, "beta": [2, 3]})
    )


def test_signed_bundle_inventory_matches_rust_commitment_bytes() -> None:
    fixture = object_value(fixtures()["bundle_manifest"])
    sample = object_value(fixture["sample"])
    files = cast(list[Json], sample["files"])
    first_file = object_value(files[0])
    assert fixture["version"] == "babble.canonical.v1"
    assert canonical_value_hex(sample) == fixture["bytes_hex"]
    first_file["size_bytes"] = cast(int, first_file["size_bytes"]) + 1
    assert canonical_value_hex(sample) != fixture["bytes_hex"]


def test_canonical_wrappers_reject_ambiguous_runtime_values() -> None:
    with pytest.raises(ValueError, match="canonical float must be numeric"):
        _ = canonical_float(cast(object, "1.0"))
    with pytest.raises(ValueError, match="canonical float must be numeric"):
        _ = canonical_float(True)
    with pytest.raises(ValueError, match="canonical unsigned integer must be an integer"):
        _ = canonical_unsigned(True)
    with pytest.raises(ValueError, match="canonical unsigned integer must be an integer"):
        _ = canonical_unsigned(cast(object, 1.5))


def test_canonical_encoder_rejects_unsupported_runtime_values() -> None:
    with pytest.raises(ValueError, match="canonical object keys must be strings"):
        _ = canonical_value_bytes(cast(CanonicalValue, {1: "value"}))
    with pytest.raises(ValueError, match="unsupported canonical value: object"):
        _ = canonical_value_bytes(cast(CanonicalValue, object()))
    with pytest.raises(ValueError, match="unsupported canonical value: bytes"):
        _ = canonical_value_bytes(cast(CanonicalValue, b"bytes"))
    with pytest.raises(ValueError, match="canonical string must be valid UTF-8"):
        _ = canonical_value_bytes("\ud800")


def test_canonical_unsigned_encodes_u64_boundary() -> None:
    assert canonical_value_hex(canonical_unsigned(18_446_744_073_709_551_615)).startswith(
        "626162626c652e63616e6f6e6963616c2e76310075"
    )
    with pytest.raises(ValueError, match="canonical unsigned integer must fit in u64"):
        _ = canonical_unsigned(18_446_744_073_709_551_616)
