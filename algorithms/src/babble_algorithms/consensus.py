from __future__ import annotations

import math
from dataclasses import dataclass, field
from enum import StrEnum
from typing import ClassVar, Literal, get_args

from babble_algorithms.text import jaccard, sentences, tokens, top_terms
from babble_algorithms.types import clamp_score

SourceKind = Literal[
    "official_docs",
    "research_paper",
    "technical_blog",
    "community_wiki",
    "forum_post",
    "social_media",
    "context",
]

MAX_ID_BYTES = 512
MAX_SOURCES = 200
MAX_SOURCE_TEXT_BYTES = 64 * 1024
MAX_TOTAL_TEXT_BYTES = 512 * 1024
MIN_TIMESTAMP = -62_167_219_200
MAX_TIMESTAMP = 253_402_300_800


def _text_size(value: object, name: str, limit: int, *, nonblank: bool = False) -> int:
    if not isinstance(value, str):
        raise ValueError(f"{name} must be a string")
    if len(value) > limit:
        raise ValueError(f"{name} must be at most {limit} UTF-8 bytes")
    if nonblank and not value.strip():
        raise ValueError(f"{name} must be nonblank")
    try:
        size = len(value.encode("utf-8"))
    except UnicodeEncodeError as error:
        raise ValueError(f"{name} must be valid UTF-8") from error
    if size > limit:
        raise ValueError(f"{name} must be at most {limit} UTF-8 bytes")
    return size


def _number(value: object, name: str, *, unit: bool = False) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{name} must be a finite real number, not bool")
    try:
        number = float(value)
    except OverflowError as error:
        raise ValueError(f"{name} must be a finite real number") from error
    if not math.isfinite(number):
        raise ValueError(f"{name} must be a finite real number")
    if unit and not 0.0 <= number <= 1.0:
        raise ValueError(f"{name} must be in [0, 1]")
    return number


def _timestamp(value: object, name: str) -> float:
    number = _number(value, name)
    if not MIN_TIMESTAMP <= number < MAX_TIMESTAMP:
        raise ValueError(f"{name} must be in [{MIN_TIMESTAMP}, {MAX_TIMESTAMP})")
    return number


class ConsensusState(StrEnum):
    EMERGING = "emerging"
    PROVISIONAL = "provisional"
    ESTABLISHED = "established"
    CONTESTED = "contested"
    REVOKED = "revoked"
    INSUFFICIENT = "insufficient"


@dataclass(frozen=True, slots=True)
class ConsensusSource:
    """Bounded source evidence; IDs are opaque nonblank strings, not object IDs.

    Scores are finite Python int/float values in [0, 1], excluding bool.
    Timestamps are Unix seconds in the range [year 0000, year 10000). A named user
    must be authenticated by the caller; this pure library cannot authenticate
    identities. Anonymous votes are accepted as data but excluded from scoring.
    """

    source_id: str
    kind: SourceKind
    text: str
    timestamp: float
    quality_score: float = 0.5
    evidence_score: float = 0.5
    user_id: str | None = None
    vote: float | None = None
    is_context: bool = False

    def __post_init__(self) -> None:
        _text_size(self.source_id, "source_id", MAX_ID_BYTES, nonblank=True)
        if self.user_id is not None:
            _text_size(self.user_id, "user_id", MAX_ID_BYTES, nonblank=True)
        if self.kind not in get_args(SourceKind):
            raise ValueError("kind must be a SourceKind literal")
        _text_size(self.text, "text", MAX_SOURCE_TEXT_BYTES)
        object.__setattr__(self, "timestamp", _timestamp(self.timestamp, "timestamp"))
        object.__setattr__(
            self, "quality_score", _number(self.quality_score, "quality_score", unit=True)
        )
        object.__setattr__(
            self, "evidence_score", _number(self.evidence_score, "evidence_score", unit=True)
        )
        if self.vote is not None:
            object.__setattr__(self, "vote", _number(self.vote, "vote", unit=True))
        if type(self.is_context) is not bool:
            raise ValueError("is_context must be bool")


@dataclass(frozen=True, slots=True)
class ConsensusResult:
    content_id: str
    consensus_score: float
    reliability_score: float
    validation_count: int
    state: ConsensusState
    temporal_weight: float
    term_agreement: float
    fact_agreement: float
    user_contributions: dict[str, float] = field(default_factory=dict)


class ConsensusAnalyzer:
    """Deterministic lexical consensus signals, never a truth assessment.

    Sources have equal weight in lexical comparisons and reliability averaging.
    Source-kind weights are unverified priors, not verified provenance or authority.
    Vote evidence first averages each named user's votes, then averages users
    equally. With no eligible voters the vote component retains its neutral 0.5
    prior; user_contributions is empty, representing absence of voter evidence.
    Contributions average that user's per-source time-weighted agreement
    with the final score, considering only sources carrying a vote. Distinct
    source IDs are required; repeated text is not proof of independent evidence.
    """

    source_weights: ClassVar[dict[SourceKind, float]] = {
        "official_docs": 1.0,
        "research_paper": 0.92,
        "technical_blog": 0.8,
        "community_wiki": 0.7,
        "forum_post": 0.58,
        "social_media": 0.38,
        "context": 0.62,
    }
    high = 0.8
    medium = 0.6
    low = 0.4

    def evaluate(
        self,
        content_id: str,
        sources: tuple[ConsensusSource, ...],
        *,
        reference_time: float,
        previous_score: float | None = None,
    ) -> ConsensusResult:
        """Evaluate at most 200 sources with at most 512 KiB of total UTF-8 text.

        Empty tuples are valid absence of evidence; null is invalid. Sources
        dated after reference_time are rejected, including when other sources
        are old. The oldest source determines the overall seven-day decay.
        """
        _text_size(content_id, "content_id", MAX_ID_BYTES, nonblank=True)
        reference_time = _timestamp(reference_time, "reference_time")
        if previous_score is not None:
            previous_score = _number(previous_score, "previous_score", unit=True)
        if type(sources) is not tuple:
            raise ValueError("sources must be a tuple of ConsensusSource values")
        if len(sources) > MAX_SOURCES:
            raise ValueError(f"sources must contain at most {MAX_SOURCES} entries")
        total_text_bytes = 0
        for source in sources:
            if type(source) is not ConsensusSource:
                raise ValueError("sources must contain ConsensusSource values")
            total_text_bytes += _text_size(source.text, "text", MAX_SOURCE_TEXT_BYTES)
            if total_text_bytes > MAX_TOTAL_TEXT_BYTES:
                raise ValueError("total source text must be at most 512 KiB of UTF-8")
            if source.timestamp > reference_time:
                raise ValueError("source timestamp must not be after reference_time")
        if len({source.source_id for source in sources}) != len(sources):
            raise ValueError("consensus source IDs must be unique")
        if not sources:
            return ConsensusResult(
                content_id,
                0.0,
                0.0,
                0,
                self._state(0.0, previous_score),
                0.0,
                0.0,
                0.0,
            )

        sources = tuple(sorted(sources, key=lambda source: source.source_id))
        voters: dict[str, list[ConsensusSource]] = {}
        for source in sources:
            if source.user_id is not None and source.vote is not None:
                voters.setdefault(source.user_id, []).append(source)

        term_agreement = self._term_agreement(sources)
        fact_agreement = self._fact_agreement(sources)
        reliability = self._reliability(sources)
        temporal_weight = self._temporal_weight(
            min(source.timestamp for source in sources), reference_time
        )
        vote_score = self._vote_score(voters)
        consensus = clamp_score(
            (0.32 * term_agreement + 0.42 * fact_agreement + 0.16 * reliability + 0.1 * vote_score)
            * (0.72 + 0.28 * temporal_weight)
        )
        contributions = {
            user_id: math.fsum(
                self._user_contribution(source, consensus, reference_time)
                for source in voters[user_id]
            )
            / len(voters[user_id])
            for user_id in sorted(voters)
        }
        return ConsensusResult(
            content_id=content_id,
            consensus_score=consensus,
            reliability_score=reliability,
            validation_count=len(sources),
            state=self._state(consensus, previous_score),
            temporal_weight=temporal_weight,
            term_agreement=term_agreement,
            fact_agreement=fact_agreement,
            user_contributions=contributions,
        )

    def _reliability(self, sources: tuple[ConsensusSource, ...]) -> float:
        return math.fsum(
            0.42 * self.source_weights["context" if source.is_context else source.kind]
            + 0.3 * source.quality_score
            + 0.28 * source.evidence_score
            for source in sources
        ) / len(sources)

    def _term_agreement(self, sources: tuple[ConsensusSource, ...]) -> float:
        term_sets = [set(top_terms(source.text, limit=12)) for source in sources]
        return _pairwise_average(term_sets)

    def _fact_agreement(self, sources: tuple[ConsensusSource, ...]) -> float:
        """Mean pairwise Jaccard overlap of indicator-bearing sentence vocabulary.

        Legacy fuzzy matching used token Jaccard with a 0.7 threshold; the prior
        typed implementation instead matched entire normalized sentences exactly.
        This continuous lexical score pools non-stopword tokens from sentences
        containing is/are/was/were/has/have/can/will/must/should. Tokens are NFC
        normalized and casefolded; numbers and negation words remain. Empty sets
        score zero, including two empty sets. Sentence repetition adds no weight.

        Pooling bounds work by input vocabulary and source pairs, avoiding a
        quadratic sentence cross-product. Word order and sentence boundaries are
        lost: paraphrases, contradictions, entity roles and truth are NOT inferred.
        Opposite claims can therefore have high lexical overlap.
        """
        fact_sets = [self._facts(source.text) for source in sources]
        return _pairwise_average(fact_sets)

    def _facts(self, text: str) -> set[str]:
        indicators = {"is", "are", "was", "were", "has", "have", "can", "will", "must", "should"}
        facts: set[str] = set()
        for sentence in sentences(text):
            sentence_tokens = set(tokens(sentence, remove_stop_words=False))
            if sentence_tokens & indicators:
                facts.update(tokens(sentence))
        return facts

    def _vote_score(self, voters: dict[str, list[ConsensusSource]]) -> float:
        if not voters:
            return 0.5
        return math.fsum(
            math.fsum(source.vote for source in voters[user_id] if source.vote is not None)
            / len(voters[user_id])
            for user_id in sorted(voters)
        ) / len(voters)

    def _temporal_weight(self, timestamp: float, reference_time: float) -> float:
        age_seconds = max(0.0, reference_time - timestamp)
        half_life = 7.0 * 24.0 * 3600.0
        return clamp_score(math.pow(2.0, -age_seconds / half_life))

    def _user_contribution(
        self,
        source: ConsensusSource,
        final_consensus: float,
        reference_time: float,
    ) -> float:
        vote = 0.5 if source.vote is None else clamp_score(source.vote)
        agreement = 1.0 - abs(final_consensus - vote)
        return clamp_score(self._temporal_weight(source.timestamp, reference_time) * agreement)

    def _state(self, current_score: float, previous_score: float | None) -> ConsensusState:
        if previous_score is not None and previous_score >= self.high:
            if current_score < self.low:
                return ConsensusState.REVOKED
            if current_score < self.medium:
                return ConsensusState.CONTESTED
            return ConsensusState.ESTABLISHED
        if current_score >= self.high:
            return ConsensusState.ESTABLISHED
        if current_score >= self.medium:
            return ConsensusState.PROVISIONAL
        if current_score >= self.low:
            return ConsensusState.EMERGING
        return ConsensusState.INSUFFICIENT


def _pairwise_average(term_sets: list[set[str]]) -> float:
    """Stream unordered pair scores with constant extra accumulation storage."""
    if len(term_sets) < 2:
        return 0.0
    pair_count = len(term_sets) * (len(term_sets) - 1) // 2
    return (
        math.fsum(
            jaccard(left, term_sets[right_index])
            for left_index, left in enumerate(term_sets)
            for right_index in range(left_index + 1, len(term_sets))
        )
        / pair_count
    )
