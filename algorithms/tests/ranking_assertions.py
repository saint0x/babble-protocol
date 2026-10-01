"""Compare Rust/Python protocol fixtures with exact structure and tight float tolerance."""

import pytest

from babel_algorithms.wire import Json


def assert_json_close(actual: Json, expected: Json) -> None:
    if isinstance(expected, dict):
        assert isinstance(actual, dict) and actual.keys() == expected.keys()
        for key, value in expected.items():
            assert_json_close(actual[key], value)
    elif isinstance(expected, list):
        assert isinstance(actual, list) and len(actual) == len(expected)
        for left, right in zip(actual, expected, strict=True):
            assert_json_close(left, right)
    elif isinstance(expected, float):
        assert isinstance(actual, (int, float)) and not isinstance(actual, bool)
        assert actual == pytest.approx(expected, rel=1e-13, abs=1e-14)
    else:
        assert actual == expected
