from __future__ import annotations

from dataclasses import dataclass
from typing import ClassVar

from babble_algorithms.text import sentences, tokens, top_terms
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
        words = tokens(text)
        sentence_values = sentences(text)
        properties = TextProperties(
            sentence_count=len(sentence_values),
            word_count=len(words),
            unique_words=len(set(words)),
            avg_sentence_length=len(words) / max(1, len(sentence_values)),
            vocabulary_richness=len(set(words)) / max(1, len(words)),
        )
        evidence = self._evidence(text)
        return ContentAnalysis(
            content_id=content_id,
            properties=properties,
            topics=self._topics(words),
            evidence=evidence,
            complexity_score=self._complexity(words, sentence_values),
            sentiment_score=self._sentiment(words),
            summary=self._summary(sentence_values, evidence.references),
            key_terms=top_terms(text),
        )

    def _evidence(self, text: str) -> EvidenceAnalysis:
        lowered = text.casefold()
        markers = tuple(marker for marker in self.evidence_markers if marker in lowered)
        references = tuple(
            sentence
            for sentence in sentences(text)
            if any(marker in sentence.casefold() for marker in markers)
        )
        return EvidenceAnalysis(
            count=len(markers),
            strength_score=clamp_score(len(markers) / 5.0 + min(0.25, len(references) * 0.05)),
            markers_found=markers,
            references=references,
        )

    def _topics(self, words: tuple[str, ...]) -> dict[str, float]:
        word_set = set(words)
        raw = {
            topic: len(word_set & topic_words) / max(1, len(topic_words))
            for topic, topic_words in self.topic_terms.items()
        }
        total = sum(raw.values())
        if total == 0.0:
            return {topic: 0.0 for topic in raw}
        return {topic: score / total for topic, score in raw.items()}

    def _complexity(self, words: tuple[str, ...], sentence_values: tuple[str, ...]) -> float:
        avg_word_length = sum(len(word) for word in words) / max(1, len(words))
        avg_sentence_length = len(words) / max(1, len(sentence_values))
        unique_ratio = len(set(words)) / max(1, len(words))
        return clamp_score(
            0.12 * avg_word_length + 0.015 * avg_sentence_length + 0.45 * unique_ratio
        )

    def _sentiment(self, words: tuple[str, ...]) -> float:
        if not words:
            return 0.0
        positives = len(set(words) & self.positive_terms)
        negatives = len(set(words) & self.negative_terms)
        return max(-1.0, min(1.0, (positives - negatives) / max(1, positives + negatives)))

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
