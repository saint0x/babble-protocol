from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
from typing import ClassVar

from babble_algorithms.text import sentences, tokens
from babble_algorithms.types import clamp_score


@dataclass(frozen=True, slots=True)
class TextProperties:
    sentence_count: int
    word_count: int
    unique_words: int
    avg_sentence_length: float
    vocabulary_richness: float


@dataclass(frozen=True, slots=True)
class EvidenceAnalysis:
    count: int
    strength_score: float
    markers_found: tuple[str, ...]
    references: tuple[str, ...]


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


def _content_id(value: object) -> str:
    if not isinstance(value, str) or not value.strip() or any(ch.isspace() for ch in value):
        raise ValueError("content_id must be a non-empty identifier without whitespace")
    return value


def _text(value: object) -> str:
    if not isinstance(value, str):
        raise ValueError("content text must be a string")
    return value
