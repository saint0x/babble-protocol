from __future__ import annotations

import math
from collections import Counter
from dataclasses import dataclass
from typing import ClassVar, cast

from babble_algorithms.text import sentences, tokens
from babble_algorithms.types import clamp_score


@dataclass(frozen=True, slots=True)
class TextProperties:
    sentence_count: int
    word_count: int
    unique_words: int
    avg_sentence_length: float
    vocabulary_richness: float

    def __post_init__(self) -> None:
        sentence_count = _count(self.sentence_count, "sentence_count")
        word_count = _count(self.word_count, "word_count")
        unique_words = _count(self.unique_words, "unique_words")
        if unique_words > word_count:
            raise ValueError("unique_words cannot exceed word_count")
        if sentence_count == 0 and word_count != 0:
            raise ValueError("word_count requires at least one sentence")
        object.__setattr__(self, "avg_sentence_length", _nonnegative_number(
            self.avg_sentence_length, "avg_sentence_length"
        ))
        object.__setattr__(self, "vocabulary_richness", _unit_score(
            self.vocabulary_richness, "vocabulary_richness"
        ))


@dataclass(frozen=True, slots=True)
class EvidenceAnalysis:
    count: int
    strength_score: float
    markers_found: tuple[str, ...]
    references: tuple[str, ...]

    def __post_init__(self) -> None:
        count = _count(self.count, "evidence count")
        object.__setattr__(self, "strength_score", _unit_score(
            self.strength_score, "evidence strength_score"
        ))
        _ = _string_tuple(self.markers_found, "evidence markers_found", nonblank=True)
        _ = _string_tuple(self.references, "evidence references")
        if count != len(self.markers_found):
            raise ValueError("evidence count must match markers_found")


@dataclass(frozen=True, slots=True)
class ContentAnalysis:
    content_id: str
    properties: TextProperties
    topics: dict[str, float]
    evidence: EvidenceAnalysis
    complexity_score: float
    sentiment_score: float
    summary: str
    key_terms: tuple[str, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "content_id", _content_id(self.content_id))
        if type(self.properties) is not TextProperties:
            raise ValueError("content properties must be TextProperties")
        if type(self.evidence) is not EvidenceAnalysis:
            raise ValueError("content evidence must be EvidenceAnalysis")
        object.__setattr__(self, "topics", _topics(self.topics))
        object.__setattr__(self, "complexity_score", _unit_score(
            self.complexity_score, "complexity_score"
        ))
        object.__setattr__(self, "sentiment_score", _bounded_number(
            self.sentiment_score, "sentiment_score", minimum=-1.0, maximum=1.0
        ))
        object.__setattr__(self, "summary", _text(self.summary))
        _ = _string_tuple(self.key_terms, "key_terms", nonblank=True)


@dataclass(frozen=True, slots=True)
class _ContentFeatures:
    lowered: str
    words: tuple[str, ...]
    word_set: set[str]
    sentence_values: tuple[str, ...]
    lowered_sentence_values: tuple[str, ...]


class ContentAnalyzer:
    evidence_markers: ClassVar[tuple[str, ...]] = (
        "according to",
        "research shows",
        "study finds",
        "evidence suggests",
        "data indicates",
        "experts say",
        "analysis reveals",
        "investigation shows",
        "dataset",
        "doi",
        "citation",
        "methodology",
        "replication",
    )
    topic_terms: ClassVar[dict[str, frozenset[str]]] = {
        "technology": frozenset({"ai", "software", "digital", "computer", "protocol", "crypto"}),
        "science": frozenset({"research", "study", "data", "experiment", "replication"}),
        "economics": frozenset({"market", "price", "trade", "cost", "incentive"}),
        "health": frozenset({"health", "clinical", "patient", "medicine", "risk"}),
        "culture": frozenset({"art", "music", "film", "culture", "creative"}),
        "governance": frozenset({"policy", "vote", "community", "moderation", "governance"}),
    }
    positive_terms: ClassVar[frozenset[str]] = frozenset(
        {"good", "useful", "supports", "confirms", "constructive", "works"}
    )
    negative_terms: ClassVar[frozenset[str]] = frozenset(
        {"bad", "harm", "hate", "fails", "contradicts", "broken", "attack"}
    )

    def analyze(self, content_id: str, text: str) -> ContentAnalysis:
        content_id = _content_id(content_id)
        features = self._features(_text(text))
        words = features.words
        sentence_values = features.sentence_values
        unique_words = len(features.word_set)
        properties = TextProperties(
            sentence_count=len(sentence_values),
            word_count=len(words),
            unique_words=unique_words,
            avg_sentence_length=len(words) / max(1, len(sentence_values)),
            vocabulary_richness=unique_words / max(1, len(words)),
        )
        evidence = self._evidence(features)
        return ContentAnalysis(
            content_id=content_id,
            properties=properties,
            topics=self._topics(features),
            evidence=evidence,
            complexity_score=self._complexity(features),
            sentiment_score=self._sentiment(features),
            summary=self._summary(sentence_values, evidence.references),
            key_terms=self._top_terms(words),
        )

    def _features(self, text: str) -> _ContentFeatures:
        words = tokens(text)
        sentence_values = sentences(text)
        return _ContentFeatures(
            text.casefold(),
            words,
            set(words),
            sentence_values,
            tuple(sentence.casefold() for sentence in sentence_values),
        )

    def _evidence(self, features: _ContentFeatures) -> EvidenceAnalysis:
        markers = tuple(marker for marker in self.evidence_markers if marker in features.lowered)
        references = tuple(
            sentence
            for sentence, lowered in zip(
                features.sentence_values, features.lowered_sentence_values, strict=True
            )
            if any(marker in lowered for marker in markers)
        )
        return EvidenceAnalysis(
            count=len(markers),
            strength_score=clamp_score(len(markers) / 5.0 + min(0.25, len(references) * 0.05)),
            markers_found=markers,
            references=references,
        )

    def _topics(self, features: _ContentFeatures) -> dict[str, float]:
        raw = {
            topic: len(features.word_set & topic_words) / max(1, len(topic_words))
            for topic, topic_words in self.topic_terms.items()
        }
        total = sum(raw.values())
        if total == 0.0:
            return {topic: 0.0 for topic in raw}
        return {topic: score / total for topic, score in raw.items()}

    def _complexity(self, features: _ContentFeatures) -> float:
        words = features.words
        avg_word_length = sum(len(word) for word in words) / max(1, len(words))
        avg_sentence_length = len(words) / max(1, len(features.sentence_values))
        unique_ratio = len(features.word_set) / max(1, len(words))
        return clamp_score(
            0.12 * avg_word_length + 0.015 * avg_sentence_length + 0.45 * unique_ratio
        )

    def _sentiment(self, features: _ContentFeatures) -> float:
        if not features.words:
            return 0.0
        positives = len(features.word_set & self.positive_terms)
        negatives = len(features.word_set & self.negative_terms)
        return max(-1.0, min(1.0, (positives - negatives) / max(1, positives + negatives)))

    def _top_terms(self, words: tuple[str, ...], *, limit: int = 10) -> tuple[str, ...]:
        return tuple(term for term, _count in Counter(words).most_common(limit))

    def _summary(self, sentence_values: tuple[str, ...], references: tuple[str, ...]) -> str:
        if not sentence_values:
            return ""
        selected = [sentence_values[0]]
        for reference in references:
            if reference not in selected:
                selected.append(reference)
            if len(selected) >= 3:
                break
        return ". ".join(selected) + "."


def _count(value: object, name: str) -> int:
    if type(value) is not int or value < 0:
        raise ValueError(f"{name} must be a nonnegative integer")
    return value


def _bounded_number(value: object, name: str, *, minimum: float, maximum: float) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise ValueError(f"{name} must be a finite number")
    number = float(value)
    if not math.isfinite(number) or not minimum <= number <= maximum:
        raise ValueError(f"{name} must be in [{minimum}, {maximum}]")
    return number


def _nonnegative_number(value: object, name: str) -> float:
    return _bounded_number(value, name, minimum=0.0, maximum=math.inf)


def _unit_score(value: object, name: str) -> float:
    return _bounded_number(value, name, minimum=0.0, maximum=1.0)


def _string_tuple(value: object, name: str, *, nonblank: bool = False) -> tuple[str, ...]:
    if type(value) is not tuple:
        raise ValueError(f"{name} must be a tuple")
    items = cast(tuple[object, ...], value)
    strings: list[str] = []
    for item in items:
        if not isinstance(item, str):
            raise ValueError(f"{name} entries must be strings")
        text = _text(item)
        if nonblank and not text.strip():
            raise ValueError(f"{name} entries must be nonblank")
        strings.append(text)
    return tuple(strings)


def _topics(value: object) -> dict[str, float]:
    if type(value) is not dict:
        raise ValueError("topics must be a dict")
    values = cast(dict[object, object], value)
    topics: dict[str, float] = {}
    for topic, score in values.items():
        if not isinstance(topic, str) or not topic.strip():
            raise ValueError("topic names must be nonblank strings")
        topics[topic] = _unit_score(score, "topic score")
    return topics


def _content_id(value: object) -> str:
    if not isinstance(value, str) or not value.strip() or any(ch.isspace() for ch in value):
        raise ValueError("content_id must be a non-empty identifier without whitespace")
    return value


def _text(value: object) -> str:
    if not isinstance(value, str):
        raise ValueError("content text must be a string")
    return value
