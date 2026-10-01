from __future__ import annotations

import json
from pathlib import Path

from babel_algorithms import canonical_float, canonical_value_hex


def test_canonical_encoder_matches_rust_fixture_bytes() -> None:
    fixtures = json.loads(
        (Path(__file__).parents[2] / "fixtures/protocol/v1/fixtures.json").read_text()
    )
    sample = {
        "zeta": [1, "1", canonical_float(1.0)],
        "alpha": {
            "nested": True,
            "empty": None,
        },
    }

    assert fixtures["canonical_encoding"]["version"] == "babel.canonical.v1"
    assert canonical_value_hex(sample) == fixtures["canonical_encoding"]["bytes_hex"]


def test_canonical_encoder_preserves_type_boundaries_and_key_order() -> None:
    assert canonical_value_hex("1") != canonical_value_hex(1)
    assert canonical_value_hex(1) != canonical_value_hex(canonical_float(1.0))
    assert canonical_value_hex({"beta": [2, 3], "alpha": {"z": True, "a": None}}) == (
        canonical_value_hex({"alpha": {"a": None, "z": True}, "beta": [2, 3]})
    )


def test_signed_bundle_inventory_matches_rust_commitment_bytes() -> None:
    fixtures = json.loads(
        (Path(__file__).parents[2] / "fixtures/protocol/v1/fixtures.json").read_text()
    )
    fixture = fixtures["bundle_manifest"]
    assert fixture["version"] == "babel.canonical.v1"
    assert canonical_value_hex(fixture["sample"]) == fixture["bytes_hex"]
    fixture["sample"]["files"][0]["size_bytes"] += 1
    assert canonical_value_hex(fixture["sample"]) != fixture["bytes_hex"]
