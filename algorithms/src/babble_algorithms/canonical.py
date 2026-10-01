from __future__ import annotations

import math
import struct
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from typing import TypeAlias, cast

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


def canonical_float(value: object) -> CanonicalFloat:
    number = _finite_number(value, "canonical float")
    return CanonicalFloat(number)


def canonical_unsigned(value: object) -> CanonicalUnsigned:
    unsigned = _unsigned_int(value, "canonical unsigned integer")
    if unsigned > MAX_U64:
        raise ValueError(f"canonical unsigned integer must fit in u64: {value}")
    return CanonicalUnsigned(unsigned)


def canonical_value_bytes(value: CanonicalValue) -> bytes:
    out = bytearray(PREAMBLE)
    _encode_value(value, out)
    return bytes(out)


def canonical_value_hex(value: CanonicalValue) -> str:
    return canonical_value_bytes(value).hex()


def _encode_value(value: object, out: bytearray) -> None:
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
        _encode_object(cast(Mapping[object, object], value), out)
    elif isinstance(value, Sequence) and not isinstance(value, bytes | bytearray):
        out.extend(b"a")
        _encode_u64(len(value), out)
        for item in value:
            _encode_value(item, out)
    else:
        raise ValueError(f"unsupported canonical value: {type(value).__name__}")


def _encode_int(value: object, out: bytearray) -> None:
    integer = _exact_int(value, "canonical integer")
    if MIN_I64 <= integer <= MAX_I64:
        out.extend(b"i")
        out.extend(integer.to_bytes(8, "big", signed=True))
        return
    if 0 <= integer <= MAX_U64:
        out.extend(b"u")
        _encode_u64(integer, out)
        return
    raise ValueError(f"canonical integer must fit in i64 or u64: {value}")


def _encode_float(value: object, out: bytearray) -> None:
    number = _finite_number(value, "canonical float")
    out.extend(b"d")
    out.extend(struct.pack(">d", number))


def _encode_string(value: object, out: bytearray) -> None:
    if not isinstance(value, str):
        raise ValueError(f"canonical string must be str: {value}")
    try:
        encoded = value.encode("utf-8")
    except UnicodeError as error:
        raise ValueError("canonical string must be valid UTF-8") from error
    out.extend(b"s")
    _encode_u64(len(encoded), out)
    out.extend(encoded)


def _encode_object(value: Mapping[object, object], out: bytearray) -> None:
    out.extend(b"o")
    _encode_u64(len(value), out)
    entries: list[tuple[str, object]] = []
    for key, item in value.items():
        if not isinstance(key, str):
            raise ValueError(f"canonical object keys must be strings: {key}")
        entries.append((key, item))
    for key, item in sorted(entries, key=lambda entry: entry[0].encode("utf-8")):
        _encode_string(key, out)
        _encode_value(item, out)


def _encode_u64(value: object, out: bytearray) -> None:
    unsigned = _unsigned_int(value, "canonical length/integer")
    if unsigned > MAX_U64:
        raise ValueError(f"canonical length/integer must fit in u64: {value}")
    out.extend(unsigned.to_bytes(8, "big", signed=False))


def _exact_int(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{label} must be an integer: {value}")
    return value


def _unsigned_int(value: object, label: str) -> int:
    integer = _exact_int(value, label)
    if integer < 0:
        raise ValueError(f"{label} must be non-negative: {value}")
    return integer


def _finite_number(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise ValueError(f"{label} must be numeric: {value}")
    number = float(value)
    if not math.isfinite(number):
        raise ValueError(f"{label} must be finite: {value}")
    return number
