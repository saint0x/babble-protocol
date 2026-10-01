from __future__ import annotations

import json
from pathlib import Path
from typing import cast

from babble_algorithms import canonical_float, canonical_value_hex
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
