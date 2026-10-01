"""Map existing lexical algorithms to core Judgment outputs, without owning records."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal, TypeAlias

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


PROVIDER = Provider()
LEXICAL_LIMITATION = "Lexical heuristics, not trained semantic models or calibrated probabilities."


@dataclass(frozen=True, slots=True)
class ScoreOutput:
    kind: Literal["probability", "bounded_score"]
    score: float
    confidence: float
    confidence_status: Literal["legacy_heuristic"]
    label: str
    reasons: tuple[str, ...]
    limitations: tuple[str, ...]


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


Output: TypeAlias = (
    ScoreOutput | RelationshipOutput | ContentOutput | ModerationOutput | SourceAgreementOutput
)


@dataclass(frozen=True, slots=True)
class JudgeResult:
    provider: Provider
    output: Output
    confidence: float


@dataclass(frozen=True, slots=True)
class HealthResult:
    provider: Provider
    supported_definitions: tuple[Definition, ...]
    ranking_provider: RankingProvider
    temporal_provider: TemporalProvider


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
            scores.append(TemporalOutput(
                object_id=score.content_id,
                age_hours=score.age_hours,
                recency=score.recency,
                decay_rate=score.decay_rate,
                time_sensitivity=score.time_sensitivity,
                engagement_velocity=score.engagement_velocity,
                survival_score=score.survival_score,
            ))
        return TemporalResult(TEMPORAL_PROVIDER, request.reference_time, tuple(scores))

    def judge(self, request: JudgmentRequest) -> JudgeResult:
        definition = request.definition
        state = request.state
        if definition == "babble.judgment.source_agreement.v1":
            agreement = state.source_agreement
            if agreement is None:
                raise ValueError("source agreement input is required")
            result = ConsensusAnalyzer().evaluate(
                state.subject, agreement.sources,
                reference_time=agreement.reference_time, previous_score=agreement.previous_score,
            )
            output: Output = SourceAgreementOutput(
                kind="source_agreement", confidence=0.0, confidence_status="uncalibrated",
                reference_time=agreement.reference_time,
                source_ids=tuple(source.source_id for source in agreement.sources),
                content_id=result.content_id, consensus_score=result.consensus_score,
                reliability_score=result.reliability_score,
                validation_count=result.validation_count,
                state=result.state, temporal_weight=result.temporal_weight,
                term_agreement=result.term_agreement, fact_agreement=result.fact_agreement,
                user_contributions=result.user_contributions,
                limitations=(LEXICAL_LIMITATION,
                    "Agreement describes supplied sources, not truth, source independence, "
                    + "fact verification, or network consensus. No confidence estimate exists."),
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
