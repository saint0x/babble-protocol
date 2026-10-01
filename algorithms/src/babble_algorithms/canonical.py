from __future__ import annotations

import struct
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from typing import TypeAlias

PREAMBLE = b"babble.canonical.v1\0"
MAX_I64 = 9_223_372_036_854_775_807
MIN_I64 = -9_223_372_036_854_775_808
MAX_U64 = 18_446_744_073_709_551_615


@dataclass(frozen=True, slots=True)
class CanonicalFloat:
    value: float


@dataclass(frozen=True, slots=True)
class CanonicalUnsigned:
    value: int


CanonicalScalar: TypeAlias = bool | int | float | str | CanonicalFloat | CanonicalUnsigned | None
CanonicalValue: TypeAlias = (
    CanonicalScalar | Sequence["CanonicalValue"] | Mapping[str, "CanonicalValue"]
)


def canonical_float(value: float) -> CanonicalFloat:
    if not _finite(value):
        raise ValueError(f"canonical float must be finite: {value}")
    return CanonicalFloat(value)


def canonical_unsigned(value: int) -> CanonicalUnsigned:
    if value < 0 or value > MAX_U64:
        raise ValueError(f"canonical unsigned integer must fit in u64: {value}")
    return CanonicalUnsigned(value)


def canonical_value_bytes(value: CanonicalValue) -> bytes:
    out = bytearray(PREAMBLE)
    _encode_value(value, out)
    return bytes(out)


def canonical_value_hex(value: CanonicalValue) -> str:
    return canonical_value_bytes(value).hex()


def _encode_value(value: CanonicalValue, out: bytearray) -> None:
    if value is None:
        out.extend(b"n")
    elif isinstance(value, bool):
        out.extend(b"t" if value else b"f")
    elif isinstance(value, CanonicalFloat):
        _encode_float(value.value, out)
    elif isinstance(value, CanonicalUnsigned):
        out.extend(b"u")
        _encode_u64(value.value, out)
    elif isinstance(value, int):
        _encode_int(value, out)
    elif isinstance(value, float):
        _encode_float(value, out)
    elif isinstance(value, str):
        _encode_string(value, out)
    elif isinstance(value, Mapping):
        _encode_object(value, out)
    else:
        out.extend(b"a")
        _encode_u64(len(value), out)
        for item in value:
            _encode_value(item, out)


def _encode_int(value: int, out: bytearray) -> None:
    if MIN_I64 <= value <= MAX_I64:
        out.extend(b"i")
        out.extend(value.to_bytes(8, "big", signed=True))
        return
    if 0 <= value <= MAX_U64:
        out.extend(b"u")
        _encode_u64(value, out)
        return
    raise ValueError(f"canonical integer must fit in i64 or u64: {value}")


def _encode_float(value: float, out: bytearray) -> None:
    if not _finite(value):
        raise ValueError(f"canonical float must be finite: {value}")
    out.extend(b"d")
    out.extend(struct.pack(">d", value))


def _encode_string(value: str, out: bytearray) -> None:
    encoded = value.encode("utf-8")
    out.extend(b"s")
    _encode_u64(len(encoded), out)
    out.extend(encoded)


def _encode_object(value: Mapping[str, CanonicalValue], out: bytearray) -> None:
    out.extend(b"o")
    _encode_u64(len(value), out)
    for key, item in sorted(value.items(), key=lambda entry: entry[0].encode("utf-8")):
        _encode_string(key, out)
        _encode_value(item, out)


def _encode_u64(value: int, out: bytearray) -> None:
    if value < 0 or value > MAX_U64:
        raise ValueError(f"canonical length/integer must fit in u64: {value}")
    out.extend(value.to_bytes(8, "big", signed=False))


def _finite(value: float) -> bool:
    return value == value and value not in (float("inf"), float("-inf"))
