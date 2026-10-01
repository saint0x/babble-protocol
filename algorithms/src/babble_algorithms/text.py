from __future__ import annotations

import math
import re
import unicodedata
from collections import Counter

STOP_WORDS = frozenset(
    {
        "a",
        "an",
        "and",
        "are",
        "as",
        "at",
        "be",
        "but",
        "by",
        "for",
        "from",
        "has",
        "have",
        "in",
        "is",
        "it",
        "of",
        "on",
        "or",
        "that",
        "the",
        "this",
        "to",
        "was",
        "were",
        "with",
    }
)


def sentences(text: str) -> tuple[str, ...]:
    return tuple(sentence.strip() for sentence in re.split(r"[.!?]+", text) if sentence.strip())


def tokens(text: str, *, remove_stop_words: bool = True) -> tuple[str, ...]:
    words = tuple(re.findall(r"\w+", unicodedata.normalize("NFC", text.casefold())))
    if remove_stop_words:
        return tuple(word for word in words if word not in STOP_WORDS)
    return words


def keyword_counts(text: str) -> Counter[str]:
    return Counter(tokens(text))


def top_terms(text: str, *, limit: int = 10) -> tuple[str, ...]:
    counts = keyword_counts(text)
    return tuple(term for term, _count in counts.most_common(limit))


def jaccard(left: set[str], right: set[str]) -> float:
    if not left or not right:
        return 0.0
    return len(left & right) / len(left | right)


def cosine(left: tuple[float, ...], right: tuple[float, ...]) -> float:
    if len(left) != len(right) or not left:
        return 0.0
    numerator = sum(a * b for a, b in zip(left, right, strict=True))
    left_norm = math.sqrt(sum(value * value for value in left))
    right_norm = math.sqrt(sum(value * value for value in right))
    if left_norm == 0.0 or right_norm == 0.0:
        return 0.0
    return numerator / (left_norm * right_norm)


def hashed_vector(terms: tuple[str, ...], *, dimensions: int = 64) -> tuple[float, ...]:
    if dimensions <= 0:
        raise ValueError("vector dimensions must be positive")
    values = [0.0] * dimensions
    counts = Counter(terms)
    for term, count in counts.items():
        index = _fnv1a(term) % dimensions
        values[index] += float(count)
    norm = math.sqrt(sum(value * value for value in values))
    if norm == 0.0:
        return tuple(values)
    return tuple(value / norm for value in values)


def _fnv1a(value: str) -> int:
    hash_value = 0x811C9DC5
    for byte in value.encode("utf-8"):
        hash_value ^= byte
        hash_value = (hash_value * 0x01000193) & 0xFFFFFFFF
    return hash_value
