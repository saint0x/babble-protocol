"""Map existing lexical algorithms to core Judgment outputs, without owning records."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Literal, TypeAlias, cast

from babble_algorithms.agreement_wire import SourceAgreementOutput
from babble_algorithms.consensus import ConsensusAnalyzer
from babble_algorithms.content import ContentAnalyzer, TextProperties
from babble_algorithms.judgment import LocalJudgmentProvider
from babble_algorithms.moderation import CommunityModerator, ModerationAction
from babble_algorithms.ranking import rank
from babble_algorithms.ranking_time import timestamp_nanos
from babble_algorithms.ranking_types import (
    RANKING_PROVIDER,
    RankingProvider,
    RankingRequest,
    RankingResult,
)
from babble_algorithms.temporal import TemporalInput, TemporalScorer
from babble_algorithms.temporal_types import (
    TEMPORAL_PROVIDER,
    TemporalOutput,
    TemporalProvider,
    TemporalRequest,
    TemporalResult,
)
from babble_algorithms.wire import DEFINITIONS, Definition, JudgmentRequest, Relation


@dataclass(frozen=True, slots=True)
class Provider:
    provider: Literal["babble-python"] = "babble-python"
    model: Literal["lexical-v1"] = "lexical-v1"
    version: Literal["1"] = "1"

    def __post_init__(self) -> None:
        if self.provider != "babble-python" or self.model != "lexical-v1" or self.version != "1":
            raise ValueError("provider metadata must match lexical-v1")


PROVIDER = Provider()
LEXICAL_LIMITATION = "Lexical heuristics, not trained semantic models or calibrated probabilities."
SCORE_KINDS = ("probability", "bounded_score")
RELATIONS = ("supports", "contradicts", "related")
MARKER_SCOPES = ("source_text", "text")
MODERATION_ACTIONS = ("allow", "limit", "flag", "remove")
ADVISORY_ACTIONS = ("allow", "warn", "limit", "flag", "remove")


def _score(value: object, name: str, *, minimum: float = 0.0, maximum: float = 1.0) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise ValueError(f"{name} must be a finite score")
    score = float(value)
    if not math.isfinite(score) or not minimum <= score <= maximum:
        raise ValueError(f"{name} must be in [{minimum}, {maximum}]")
    return score


def _string(value: object, name: str, *, nonblank: bool = False) -> str:
    if not isinstance(value, str):
        raise ValueError(f"{name} must be a string")
    try:
        _ = value.encode("utf-8")
    except UnicodeError as error:
        raise ValueError(f"{name} must be valid UTF-8") from error
    if nonblank and not value.strip():
        raise ValueError(f"{name} must be nonblank")
    return value


def _string_tuple(value: object, name: str, *, nonblank: bool = False) -> tuple[str, ...]:
    if type(value) is not tuple:
        raise ValueError(f"{name} must be a tuple")
    values = cast(tuple[object, ...], value)
    return tuple(_string(item, f"{name} entry", nonblank=nonblank) for item in values)


def _literal(value: object, allowed: tuple[object, ...], name: str) -> object:
    if value not in allowed:
        raise ValueError(f"{name} must be supported")
    return value


@dataclass(frozen=True, slots=True)
class ScoreOutput:
    kind: Literal["probability", "bounded_score"]
    score: float
    confidence: float
    confidence_status: Literal["legacy_heuristic"]
    label: str
    reasons: tuple[str, ...]
    limitations: tuple[str, ...]

    def __post_init__(self) -> None:
        _ = _literal(self.kind, SCORE_KINDS, "score kind")
        object.__setattr__(self, "score", _score(self.score, "score"))
        object.__setattr__(self, "confidence", _score(self.confidence, "confidence"))
        if self.confidence_status != "legacy_heuristic":
            raise ValueError("confidence_status must be legacy_heuristic")
        object.__setattr__(self, "label", _string(self.label, "label", nonblank=True))
        object.__setattr__(self, "reasons", _string_tuple(self.reasons, "reasons", nonblank=True))
        object.__setattr__(
            self, "limitations", _string_tuple(self.limitations, "limitations", nonblank=True)
        )


@dataclass(frozen=True, slots=True)
class RelationshipOutput:
    kind: Literal["relationship"]
    relation: Relation
    score: float
    confidence: float
    confidence_status: Literal["legacy_heuristic"]
    observed_relation: str
    reasons: tuple[str, ...]
    target_context_evaluated: bool
    marker_scope: Literal["source_text", "text"]
    limitations: tuple[str, ...]

    def __post_init__(self) -> None:
        if self.kind != "relationship":
            raise ValueError("relationship output kind is invalid")
        _ = _literal(self.relation, RELATIONS, "relationship relation")
        object.__setattr__(self, "score", _score(self.score, "relationship score"))
        object.__setattr__(self, "confidence", _score(self.confidence, "confidence"))
        if self.confidence_status != "legacy_heuristic":
            raise ValueError("confidence_status must be legacy_heuristic")
        object.__setattr__(
            self,
            "observed_relation",
            _string(self.observed_relation, "observed_relation", nonblank=True),
        )
        object.__setattr__(self, "reasons", _string_tuple(self.reasons, "reasons", nonblank=True))
        if type(self.target_context_evaluated) is not bool:
            raise ValueError("target_context_evaluated must be bool")
        _ = _literal(self.marker_scope, MARKER_SCOPES, "marker_scope")
        object.__setattr__(
            self, "limitations", _string_tuple(self.limitations, "limitations", nonblank=True)
        )


@dataclass(frozen=True, slots=True)
class ContentOutput:
    kind: Literal["content_analysis"]
    topics: tuple[str, ...]
    evidence_markers: tuple[str, ...]
    key_terms: tuple[str, ...]
    summary: str
    sentiment: float
    confidence: float
    confidence_status: Literal["uncalibrated"]
    properties: TextProperties
    topic_scores: dict[str, float]
    complexity: float
    evidence_strength: float
    limitations: tuple[str, ...]

    def __post_init__(self) -> None:
        if self.kind != "content_analysis":
            raise ValueError("content output kind is invalid")
        object.__setattr__(self, "topics", _string_tuple(self.topics, "topics", nonblank=True))
        object.__setattr__(
            self,
            "evidence_markers",
            _string_tuple(self.evidence_markers, "evidence_markers", nonblank=True),
        )
        object.__setattr__(
            self, "key_terms", _string_tuple(self.key_terms, "key_terms", nonblank=True)
        )
        object.__setattr__(self, "summary", _string(self.summary, "summary"))
        object.__setattr__(self, "sentiment", _score(self.sentiment, "sentiment"))
        object.__setattr__(self, "confidence", _score(self.confidence, "confidence"))
        if self.confidence_status != "uncalibrated":
            raise ValueError("confidence_status must be uncalibrated")
        if type(self.properties) is not TextProperties:
            raise ValueError("properties must be TextProperties")
        object.__setattr__(self, "topic_scores", _topic_scores(self.topic_scores))
        object.__setattr__(self, "complexity", _score(self.complexity, "complexity"))
        object.__setattr__(
            self, "evidence_strength", _score(self.evidence_strength, "evidence_strength")
        )
        object.__setattr__(
            self, "limitations", _string_tuple(self.limitations, "limitations", nonblank=True)
        )


@dataclass(frozen=True, slots=True)
class ModerationOutput:
    kind: Literal["moderation"]
    action: Literal["allow", "limit", "flag", "remove"]
    advisory_action: ModerationAction
    flags: tuple[str, ...]
    spam: float
    quality: float
    safety: float
    coordination: float
    misinformation: float
    sentiment: float
    confidence: float
    confidence_status: Literal["uncalibrated"]
    reasons: tuple[str, ...]
    limitations: tuple[str, ...]

    def __post_init__(self) -> None:
        if self.kind != "moderation":
            raise ValueError("moderation output kind is invalid")
        _ = _literal(self.action, MODERATION_ACTIONS, "moderation action")
        _ = _literal(self.advisory_action, ADVISORY_ACTIONS, "advisory action")
        object.__setattr__(self, "flags", _string_tuple(self.flags, "flags", nonblank=True))
        object.__setattr__(self, "spam", _score(self.spam, "spam"))
        object.__setattr__(self, "quality", _score(self.quality, "quality"))
        object.__setattr__(self, "safety", _score(self.safety, "safety"))
        object.__setattr__(self, "coordination", _score(self.coordination, "coordination"))
        object.__setattr__(self, "misinformation", _score(self.misinformation, "misinformation"))
        object.__setattr__(self, "sentiment", _score(self.sentiment, "sentiment"))
        object.__setattr__(self, "confidence", _score(self.confidence, "confidence"))
        if self.confidence_status != "uncalibrated":
            raise ValueError("confidence_status must be uncalibrated")
        object.__setattr__(self, "reasons", _string_tuple(self.reasons, "reasons", nonblank=True))
        object.__setattr__(
            self, "limitations", _string_tuple(self.limitations, "limitations", nonblank=True)
        )


def _topic_scores(value: object) -> dict[str, float]:
    if type(value) is not dict:
        raise ValueError("topic_scores must be a dict")
    result: dict[str, float] = {}
    for topic, score in cast(dict[object, object], value).items():
        result[_string(topic, "topic", nonblank=True)] = _score(score, "topic score")
    return result


Output: TypeAlias = (
    ScoreOutput | RelationshipOutput | ContentOutput | ModerationOutput | SourceAgreementOutput
)
OUTPUT_TYPES = (
    ScoreOutput,
    RelationshipOutput,
    ContentOutput,
    ModerationOutput,
    SourceAgreementOutput,
)


@dataclass(frozen=True, slots=True)
class JudgeResult:
    provider: Provider
    output: Output
    confidence: float

    def __post_init__(self) -> None:
        if type(self.provider) is not Provider:
            raise ValueError("provider must be Provider")
        if not any(type(self.output) is output_type for output_type in OUTPUT_TYPES):
            raise ValueError("output must be a Judgment output")
        object.__setattr__(self, "confidence", _score(self.confidence, "confidence"))


@dataclass(frozen=True, slots=True)
class HealthResult:
    provider: Provider
    supported_definitions: tuple[Definition, ...]
    ranking_provider: RankingProvider
    temporal_provider: TemporalProvider

    def __post_init__(self) -> None:
        if type(self.provider) is not Provider:
            raise ValueError("provider must be Provider")
        if type(self.supported_definitions) is not tuple:
            raise ValueError("supported_definitions must be a tuple")
        for definition in self.supported_definitions:
            if definition not in DEFINITIONS:
                raise ValueError("supported_definitions must contain known definitions")
        if type(self.ranking_provider) is not RankingProvider:
            raise ValueError("ranking_provider must be RankingProvider")
        if type(self.temporal_provider) is not TemporalProvider:
            raise ValueError("temporal_provider must be TemporalProvider")


class AlgorithmExecutor:
    def __init__(self) -> None:
        self.local: LocalJudgmentProvider = LocalJudgmentProvider()
        self.content: ContentAnalyzer = ContentAnalyzer()
        self.temporal_scorer: TemporalScorer = TemporalScorer()

    def health(self) -> HealthResult:
        return HealthResult(PROVIDER, DEFINITIONS, RANKING_PROVIDER, TEMPORAL_PROVIDER)

    def rank(self, request: RankingRequest) -> RankingResult:
        return rank(request)

    def temporal(self, request: TemporalRequest) -> TemporalResult:
        reference_nanos = timestamp_nanos(request.reference_time)
        epoch_nanos = timestamp_nanos("1970-01-01T00:00:00Z")
        scores: list[TemporalOutput] = []
        for item in request.items:
            published_nanos = timestamp_nanos(item.published_at)
            # Subtract integer timestamps first; future publications explicitly have age zero.
            age_hours = max(0, reference_nanos - published_nanos) / 3_600_000_000_000
            score = self.temporal_scorer.score_at_age(
                TemporalInput(
                    content_id=item.object_id,
                    published_at=(published_nanos - epoch_nanos) / 1_000_000_000,
                    content_class=item.content_class,
                    engagement=item.engagement,
                    quality_score=item.quality_score,
                    tags=item.tags,
                ),
                age_hours=age_hours,
            )
            scores.append(
                TemporalOutput(
                    object_id=score.content_id,
                    age_hours=score.age_hours,
                    recency=score.recency,
                    decay_rate=score.decay_rate,
                    time_sensitivity=score.time_sensitivity,
                    engagement_velocity=score.engagement_velocity,
                    survival_score=score.survival_score,
                )
            )
        return TemporalResult(TEMPORAL_PROVIDER, request.reference_time, tuple(scores))

    def judge(self, request: JudgmentRequest) -> JudgeResult:
        definition = request.definition
        state = request.state
        if definition == "babble.judgment.source_agreement.v1":
            agreement = state.source_agreement
            if agreement is None:
                raise ValueError("source agreement input is required")
            result = ConsensusAnalyzer().evaluate(
                state.subject,
                agreement.sources,
                reference_time=agreement.reference_time,
                previous_score=agreement.previous_score,
            )
            output: Output = SourceAgreementOutput(
                kind="source_agreement",
                confidence=0.0,
                confidence_status="uncalibrated",
                reference_time=agreement.reference_time,
                source_ids=tuple(source.source_id for source in agreement.sources),
                content_id=result.content_id,
                consensus_score=result.consensus_score,
                reliability_score=result.reliability_score,
                validation_count=result.validation_count,
                state=result.state,
                temporal_weight=result.temporal_weight,
                term_agreement=result.term_agreement,
                fact_agreement=result.fact_agreement,
                user_contributions=result.user_contributions,
                limitations=(
                    LEXICAL_LIMITATION,
                    "Agreement describes supplied sources, not truth, source independence, "
                    + "fact verification, or network consensus. No confidence estimate exists.",
                ),
            )
        elif definition == "babble.judgment.content_analysis.v1":
            analysis = self.content.analyze(state.subject, state.text)
            output = ContentOutput(
                kind="content_analysis",
                topics=tuple(topic for topic, score in analysis.topics.items() if score > 0),
                evidence_markers=analysis.evidence.markers_found,
                key_terms=analysis.key_terms,
                summary=analysis.summary or state.text.strip(),
                sentiment=(analysis.sentiment_score + 1) / 2,
                confidence=0.0,
                confidence_status="uncalibrated",
                properties=analysis.properties,
                topic_scores=analysis.topics,
                complexity=analysis.complexity_score,
                evidence_strength=analysis.evidence.strength_score,
                limitations=(
                    LEXICAL_LIMITATION,
                    "English topic and sentiment dictionaries; extractive summary; "
                    + "evidence markers do not verify sources. No confidence estimate exists.",
                ),
            )
        elif definition == "babble.judgment.moderation.v1":
            moderation = CommunityModerator(request.parameters.policy).analyze(
                state.subject, state.text, context=request.parameters.context
            )
            output = ModerationOutput(
                kind="moderation",
                action="flag" if moderation.action == "warn" else moderation.action,
                advisory_action=moderation.action,
                flags=moderation.flags,
                spam=moderation.scores.spam,
                quality=moderation.scores.quality,
                safety=1 - moderation.scores.safety,
                coordination=moderation.scores.coordination,
                misinformation=float("misinformation_pattern" in moderation.flags),
                sentiment=(moderation.scores.sentiment + 1) / 2,
                confidence=0.0,
                confidence_status="uncalibrated",
                reasons=moderation.reasons,
                limitations=(
                    LEXICAL_LIMITATION,
                    "English marker and supplied-count rules; misinformation is a binary "
                    + "framing-marker signal, not fact checking. No confidence estimate exists.",
                ),
            )
        else:
            marker_text = state.source_text if state.source_text is not None else state.text
            judgment = self.local.judge(definition, marker_text, context=request.parameters.query)
            if definition == "babble.judgment.relationship.v1":
                relation = request.parameters.relation
                matches = judgment.label == relation or (
                    relation == "related" and judgment.label in ("supports", "contradicts")
                )
                output = RelationshipOutput(
                    kind="relationship",
                    relation=relation,
                    score=judgment.score if matches else 0.0,
                    confidence=judgment.confidence,
                    confidence_status="legacy_heuristic",
                    observed_relation=judgment.label,
                    reasons=judgment.reasons,
                    target_context_evaluated=False,
                    marker_scope="source_text" if state.source_text is not None else "text",
                    limitations=(
                        LEXICAL_LIMITATION,
                        "English markers in the reported marker_scope; text may combine "
                        + "source and target. No target comparison, negation, "
                        + "or entailment analysis. Contradiction markers take precedence. "
                        + "Related means a support or contradiction marker was observed; "
                        + "a mismatched or unknown finding scores zero for the requested relation.",
                    ),
                )
            else:
                output = ScoreOutput(
                    kind="probability"
                    if definition == "babble.judgment.spam.v1"
                    else "bounded_score",
                    score=judgment.score,
                    confidence=judgment.confidence,
                    confidence_status="legacy_heuristic",
                    label=judgment.label,
                    reasons=judgment.reasons,
                    limitations=(
                        LEXICAL_LIMITATION,
                        "English lexical markers; relevance uses ASCII token overlap with "
                        + "parameters.query only; evidence markers do not verify sources.",
                    ),
                )
        return JudgeResult(PROVIDER, output, output.confidence)
