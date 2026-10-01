# Algorithm Migration Audit

Audited 2026-09-29 against the shared working tree, with a subsequent verified
Python-worker integration and a public ranking follow-up on 2026-09-30. Use named symbols rather than
historical line numbers when navigating source.

## Coverage and Meaning

All **14 deprecated production Python modules and 96 declared functions/methods**
are accounted for below: 70 functions in the six algorithm families and 26 in
infrastructure/interface/config/HTTP modules. Counts include constructors and
validation methods, exclude imported functions, and use declaration inventory
because the old recommendation module has invalid indentation. Models with no
methods, package exports, and the script entry point are also covered. Deprecated
`test/` scripts, SQLite fixtures, generated reports, caches, and lockfiles are not
production algorithms.

This is complete **audit coverage**, not complete migration or runtime coverage.
The six families have executable Python implementations. The served API now calls
`babble_algorithms` for seven Judgment definitions, including content, moderation,
and on-demand [source agreement](source-agreement.md). This closes concrete portions
of R1/R3, not the full migration. Python private recommendation and engagement
still lack complete live consumers. Temporal scoring has a versioned public discovery consumer;
see [its contract and limits](temporal-scoring.md). Canonical public ranking runs through Python with
19 cross-language golden cases; this is distinct from the legacy private-user
recommender. Passing tests does not establish trained intelligence, calibration,
or whole-platform privacy compliance.

Status vocabulary: **implemented** means executable typed Python behavior;
**changed** means the concept exists with different rules/output; **partial** means
named sub-behavior remains absent; **retired** means legacy infrastructure is not
part of the  Python library; **missing** means no implementation of that concept.
Implemented/changed Python rows must be read against R1-R8 below; only the
explicitly verified worker slice is closed.

## Verified Worker Integration

- `backend/crates/judgment-python` calls the installed isolated Python package;
  `backend/crates/api/src/provider.rs` selects it by default. Startup health,
  bounded frames/deadlines, strict validation, fault restart, minimal environment,
  and real-worker conformance are implemented. See [the worker contract](algorithm-worker.md).
- `worker.py`, `wire.py`, and `execution.py` connect the existing lexical Judgment,
  content, and moderation algorithms. Rust owns the record, ID, input hash,
  parameter-bound commitment, timestamp, validation, and cache. No output is
  silently substituted from a different provider.
- Publication preflights all four ingestion Judgments before durable Object/event
  writes. Provider-error tests cover text, drafts, media, signed records, replies,
  shares, forks, and remixes. Subsequent publication work now commits these
  records as one durable batch before updating in-memory state, with redo-journal
  recovery; the former nontransactional-publication gap is superseded.
- The original API/Astro/Aegis worker run verifies its six initial definitions, publication,
  repeated evaluations, provider readback, and heuristic/uncalibrated UI labels.
  `artifacts/live-stack-python-verified-host.trace.fozzy` passed strict verification,
  replay, and CI. Python has 155 passing tests; configured strict type checking
  and lint pass. Worker tests include Rust-exported fixtures.
- Content sentiment and safety are explicitly mapped into the core score ranges;
  moderation `warn` remains visible as an advisory action while mapping to `flag`.
  These changes do not turn English lexical rules into semantic models.
- Public ranking now has canonical Candidate/Lens/RankingTrace equivalence across
  19 cases, a bounded real-worker path, full-batch and fault tests, and provider
  provenance. Rust admits candidates fairly with complete selected provenance;
  Python executes the eight public Lens rules and soft source diversity.
  [Public ranking](public-ranking.md) records the boundary and remaining limits.
- The browser now reapplies canonical soft diversity after private filtering and
  scoring, before selecting nine cards. Native fixture comparisons and client
  tests pass; focused live acceptance is tracked in [public ranking](public-ranking.md).
- R2-R7 remain partly open: retrieval/safety policy evaluation, semantic models,
  other consumers and private adaptation are not closed by these ranking
  milestones. R8 now includes ranking wire conformance.

Evidence shorthand refers to files under `algorithms/`:

- **A**: `tests/test_algorithms.py`, 11 existing family/ranking behavior tests.
- **C**: `tests/test_canonical.py`, two encoding fixture/type-boundary tests.
- **R**: `tests/test_regressions.py`, 17 cases for invalid numeric inputs, historical
  recommendations, explicit discovery signals, consensus, Lens normalization, and Unicode.
- **F**: `tests/test_feedback.py`, 14 cases including actual recommendation order
  changes, immutability, sparse/neutral feedback, invalid inputs, and repeated updates.
- A source symbol is implementation evidence only; a row without a relevant test
  explicitly says so. These tests do not constitute a semantic evaluation corpus.

## Content Analysis: 10 Functions

Legacy `deprecated/algorithms-deprec/content_analysis.py` maps to
`algorithms/src/babble_algorithms/content.py` and `text.py`.

| Legacy functions/models | Actual Python behavior and evidence | Status / remaining work |
| --- | --- | --- |
| `ContentAnalysisResult` | `ContentAnalysis`, `TextProperties`, `EvidenceAnalysis`: typed results, no metadata blob, wall-clock timestamp, or base response envelope. | Changed; `execution.py` maps fields into the validated core output; Rust creates the Judgment. |
| `__init__`, `validate_input`, `process` | Stateless `ContentAnalyzer.analyze(content_id, text)`; explicit typed arguments; no NLTK downloads/cache/network. | Changed; strict runtime JSON validation/limits are now in `wire.py`; worker tests exercise content output. Semantic evaluation remains R4/R7. |
| `_tokenize` | `text.tokens`: casefold, NFC normalization, Unicode word matching, small English stop-word set. R checks accented/CJK/Cyrillic preservation. | Implemented lexical tokenizer; no multilingual segmentation/model. |
| `_analyze_text_properties` | `analyze` counts sentences and stop-word-filtered words, unique terms, mean sentence length, vocabulary richness. | Implemented with changed stop-word dictionary; direct edge-case coverage missing. |
| `_analyze_semantics` | `_complexity` uses length/diversity formula; `_sentiment` uses six positive and seven negative terms. | Partial: not VADER parity, negation-aware sentiment, entity extraction, or semantic understanding. Old `entities=[]` was itself a placeholder. R4. |
| `_classify_topics` | `_topics` uses six fixed dictionaries and normalized overlap. Legacy executed only technology/science despite listing ten categories. | Changed; no trained topic classifier. R4 requires definition-specific evaluation. |
| `_analyze_evidence`, `_extract_context` | `_evidence` counts marker types and extracts matching sentences into `EvidenceAnalysis.references`. | Implemented lexical cues; does not verify citations, source trust, or entailment. R3/R4. |
| `_generate_summary` | `_summary` selects the first sentence plus at most two distinct evidence sentences; empty input returns empty string. | Implemented extractive heuristic, including short text. No abstractive summarizer, entity/context model, or summary calibration. |

## Moderation: 12 Functions

Legacy `community_moderation.py` maps to `moderation.py` plus `ContentAnalyzer`.

| Legacy functions/models | Actual Python behavior and evidence | Status / remaining work |
| --- | --- | --- |
| `ModerationResult` | `ModerationResult`, `ModerationScores`, `ModerationContext`, `ModerationPolicy`: flags, reasons, action, five scores. | Changed; this library remains advisory. The separate [review workflow](moderation.md) now persists policy-versioned decisions, private audit receipts, appeals and node-local enforcement. Semantic policy quality remains R3/R4. |
| `__init__`, `validate_input`, `process` | `CommunityModerator.__init__`/`analyze` execute spam, quality, safety, coordination and action rules. A and worker tests. | Implemented heuristic composition; domain policy/context range checks and strict wire validation exist. Human-reviewed enforcement and appeals are integrated separately; model outputs never automatically become restrictions. Semantic accuracy remains open. |
| `_tokenize` | Shared Unicode `text.tokens`. | Implemented; only English moderation marker dictionaries. |
| `check_spam_score` | `_spam_score` uses promotional/urgency phrases, links, email, repetition, capitalization, repeated messages, punctuation. | Changed; no legacy catch-all returning a fake zero, and capitalization inspects original case. A tests high spam. |
| `_assess_quality`, `_assess_formatting` | `_quality_score` combines text length, sentence capitalization, complexity, evidence and negative sentiment penalty. | Partial parity: old paragraph-break/excess-caps formatting penalties are absent from quality; caps still affect spam. Need policy fixtures before selecting intended rules. R3. |
| `_assess_engagement_potential` | Quality incorporates evidence/sentiment, but no question-count engagement bonus or distinct potential output. | Partial; decide policy explicitly, do not label the replacement identical. |
| `_analyze_sentiment` | `ContentAnalyzer._sentiment`. | Changed from VADER to a tiny lexicon; no semantic toxicity claim. R4. |
| `_contains_hate_speech` | `_safety_score` counts abusive words/threat phrases; `_action` prioritizes high safety score. | Changed, not a hate-speech model; quotation/negation/context are not resolved. No dedicated safety-precedence test. R3/R4. |
| `_check_misinformation` | Six lexical framing markers trigger `misinformation_pattern`. | Changed; reports affect coordination instead. No fact checking, source comparison or misinformation probability. |
| `_check_coordinated_behavior` | `_coordination_score` combines reports, similar-post count and young-account flag. | Partial: no explicit coordinated-account flag or validated time-window model. Caller must supply meaningful recent counts. |

Actions are recommendations from this library. Connecting `remove` directly to
irreversible protocol deletion would violate the spec's preservation/audit rules.

## Source Agreement: 15 Functions

Legacy `consensus.py` maps to `consensus.py`. This is **content/source agreement**,
not Rust hashgraph consensus, Byzantine finality, or an authoritative truth score.

| Legacy functions/models | Actual Python behavior and evidence | Status / remaining work |
| --- | --- | --- |
| `ConsensusState`, `ConsensusResult` | All six states retained; `ConsensusSource` supplies typed source kind, text, time, quality/evidence, optional vote/user/context. | Implemented different result contract; R3. |
| `__init__`, `validate_input`, `process` | Explicit reference time/previous score; strict finite numeric, UTF-8 byte, count, unique-ID, and future-time validation; real empty input. | Changed; signed node selection and exact-copy suppression now feed the canonical worker. Distinct sources still do not prove independence. |
| `_calculate_temporal_weight` | Seven-day half-life with explicit time; future sources rejected at evaluation/wire boundaries. | Implemented deterministic time input; node also rejects future evidence records. |
| `_determine_consensus_state` | `_state` checks revocation before contestation, retains established state at scores >=0.6. | Implemented; fixes legacy unreachable revocation branch. Empty input now also observes previous-state transition. |
| `_calculate_user_contribution` | Age-weighted vote agreement averaged per named user; votes average within user, then across users. | Implemented heuristic with permutation/repeated-user coverage. Served source agreement supplies no votes or inferred voter ownership. |
| `_calculate_consensus` | `evaluate`: 0.32 term + 0.42 fact + 0.16 reliability + 0.10 vote, then age adjustment. | Changed; not legacy 0.4/0.6 weighting. Agreement requires at least two sources after this fix. |
| `_calculate_reliability` | `_reliability`: weighted kind/quality/evidence average, context kind override. | Changed weights; declared source kind is not verified trust. |
| `_extract_key_terms` | Shared `top_terms`, 12 terms rather than old ten/length>2 filter. | Changed lexical representation. |
| `_extract_facts` | `_facts` pools vocabulary from sentences containing copular/modal verbs. | Changed; grammatical marker extraction is not a fact extractor or verifier. |
| `_calculate_term_agreement` | `_term_agreement` / `_pairwise_average` average pairwise set Jaccard. | Changed from thresholded term frequency. Singleton agreement now zero. R. |
| `_calculate_fact_agreement`, `_calculate_fact_similarity` | Continuous pairwise Jaccard on pooled indicator-bearing sentence vocabulary, replacing exact sentence equality. | Changed from thresholded fuzzy matching; opposite claims may overlap highly. Not semantic entailment or paraphrase matching. R4 remains open. |
| `_calculate_source_weight` | `_reliability` handles context as a source class. | Partial: media/text-length context bonuses are absent; no real provenance verification. |
| `_calculate_consensus_score` | `evaluate` / `_vote_score` combine supplied votes with agreement/reliability. | Changed. Legacy method called nonexistent `super()._calculate_consensus_score`; its 1.5x context bonus was not a working end-to-end path. No standalone vote-only collection API. |

## Recommendation and Feedback: 17 Functions

Legacy `recommendation.py` maps to `recommendation.py`, `text.py`, `temporal.py`.

| Legacy functions/models | Actual Python behavior and evidence | Status / remaining work |
| --- | --- | --- |
| `UserProfile`, `ContentVector`, `RecommendationScore` | Immutable `UserProfile`, `Interaction`, `ContentProfile`, `RecommendationScore`; five component scores, confidence, Candidate output. | Changed. Scores are algorithm data, not signed records. R1/R5. |
| `__init__`, `validate_input`, `process` | `RecommendationEngine` normalizes validated finite non-negative weights; `recommend` takes explicit profiles/content/peers/time/limit. A recommendation ordering; R numeric/history checks. | Implemented; no legacy dictionary dispatcher, temporary-profile inference or global store. |
| `add_user`, `add_content`, `record_interaction` | Caller constructs typed profiles/content/history and passes snapshots; no hidden mutable store or wall-clock capture. | Changed to explicit data inputs; collection, persistence, identity binding and deduplication are not implemented here. R5. |
| `_process_feedback`, `_update_weights` | New `RecommendationFeedback` plus `RecommendationWeights.with_feedback`; validated explicit ratings, sparse per-dimension mean adjustment, bounded values, renormalization, immutable return. Engine consumes returned weights. F verifies actual order change. | Implemented bounded adaptation; no replay protection, per-user store, optimizer/model training, or implicit global update. R5 owns batch identity and local persistence. |
| `_create_user_vector`, `_create_content_vector` | `_relevance`/`_collaborative` use token counts hashed into 64 FNV-1a buckets and normalized. | Implemented real lexical feature vectors, replacing old constant `[0.5] * 100`; hash collisions remain, no embeddings. |
| `_calculate_recommendation_score` | `_score` blends relevance, engagement heuristic, supplied authenticity, temporal and collaborative scores, producing a Candidate. | Implemented; output source is always `Exploration`, novelty is inverse engagement. These are policy choices, not measured provenance/novelty. R2 must map correctly. |
| `_calculate_relevance_score` | `_relevance`: hashed bag-of-words cosine using interests + expertise against text + topics. | Changed from placeholder-derived similarity; no semantic/user model inference. A. |
| `_calculate_engagement_score` | `_engagement_prediction`: matching-object history mean, fixed complexity target 0.62, topic overlap. | Implemented heuristic replacing constant 0.5. Future/non-finite-timestamp interactions excluded; individual scores bounded. R. |
| `_calculate_temporal_score` | `_temporal_score` invokes `TemporalScorer` or normalized supplied recency. | Implemented replacing constant 0.5. A temporal/recommendation tests. |
| `_calculate_collaborative_score`, `_find_similar_users` | `_collaborative` weights peer matching-object history means by profile cosine; excludes self; explicit 0.5 prior if no usable neighbors. | Changed: all supplied peers, not top five; no learned embedding index. Private peer histories must not be uploaded by default. R5. |
| `_calculate_confidence` | `_confidence` adds fixed increments for presence of profile/text/topics/history/peers. | Implemented availability heuristic, **not calibrated probability**. Unrelated peers can still increase confidence. R4. |

Feedback adaptation does not reapply all historical ratings implicitly. The caller
must submit only the new batch once. A missing rating does not count as negative;
no-op/neutral feedback leaves normalized weights unchanged in covered cases. This
does not resurrect the old shared mutable user profile store.

## Temporal Scoring: 8 Functions

| Legacy `temporal_considerations.py` symbols | Current `temporal.py` behavior and evidence | Status / remaining work |
| --- | --- | --- |
| `TemporalScore`, `__init__`, `validate_input`, `process`, `_calculate_temporal_score` | `TemporalInput`, `EngagementWindow`, `ContentTimeClass`, `TemporalScorer.score` and nanosecond-safe `score_at_age`; adds survival score. | Implemented changed contract with strict domain validation, canonical batches, real evaluation clock, live discovery consumer, and inspectable components. R2 now bounds temporal enrichment through indexed admission; empirical retrieval/temporal policy evaluation remains open. |
| `_calculate_recency` | `recency` uses five class weights and age buckets. | Changed weights and inclusive boundary behavior; future publication age becomes zero. |
| `_calculate_decay_rate` | `decay_rate` combines quality, engagement velocity and time sensitivity, bounded 0.01..0.5. | Changed from old thresholded engagement/quality boosts. |
| `_calculate_time_sensitivity` | `time_sensitivity` combines class and casefolded breaking/time-sensitive/evergreen/reference tags. | Implemented changed constants/tag coverage. |
| `_calculate_engagement_velocity` | `engagement_velocity` combines recent/total view and interaction fractions with age decay; zero age returns zero. | Changed: missing interaction totals do not discard available views. Safe nonnegative integer counts, subset consistency, finite times and age boundaries are validated. Public discovery supplies inbound signed relationship counts and no view telemetry; this is not session analytics. |

## Engagement Analytics: 8 Functions

| Legacy `engagement_analytics.py` symbols | Current `engagement.py` behavior and evidence | Status / remaining work |
| --- | --- | --- |
| `EngagementMetrics`, `EngagementSummary`, `__init__`, `validate_input`, `process`, `_analyze_engagement` | `EngagementEvent`, `ContentPerformance`, `EngagementSummary`, `EngagementAnalyzer.summarize` aggregate supplied events in an inclusive explicit window; future events excluded. A segment/content test. | Changed; no event ingestion/store or retained history. Each event is counted as a session, so upstream sessionization is required. R5/R6. |
| `_analyze_peak_hours` | `_peak_hours` counts UTC hours from timestamps; returns above-average occupied hours. | Changed from caller `time_of_day`; a single occupied hour yields no peak. No timezone-aware distribution. |
| `_calculate_trend` | `_trend` returns six buckets relative to window start, uses normalized event scores and fills empty buckets with zero. | Changed from timestamp-modulo windows and duration x depth. |
| `_segment_users` | `_segments` groups mean event score at 0.8/0.45 thresholds. `_event_score` combines scroll, duration and action bonuses. | Changed from old 0.8/0.5 thresholds; descriptive heuristic. |
| `_analyze_content_performance` | `_content_performance` returns typed per-content session/scroll/duration/interaction-rate/engagement metrics. | Implemented expanded metrics. Direct invalid event/range/non-finite window tests and validation remain missing. R6. |

## Infrastructure and Interfaces: 26 Functions

| Deprecated module / every function | Current disposition | Concrete remaining action |
| --- | --- | --- |
| `base.py`: `AlgorithmMetrics`, `AlgorithmResponse`; `__init__`, `__enter__`, `__exit__` | Retired Redis/Postgres/SQLite ownership and generic Any response envelope. Dataclasses describe specific outputs. | Protocol persistence remains in Rust; the local worker supplies a real bounded lifecycle for seven definitions, not a public Python HTTP service. |
| `base.py`: `get_cache`, `set_cache` | Retired content-ID-only caching; Python calculations are stateless. Rust `JudgmentCache`/`cache_key` include definition, input hash, provider/model/version and parameter hash. | Python results now use this versioned core cache; no legacy content-ID-only cache was restored. |
| `base.py`: `record_metric`, `get_metrics`, `log_error`, `log_warning`, `execute` | Python does not retain fake success-rate/execution metrics; exceptions propagate. Rust has separate operational instrumentation. | R6 instrument the actual Python invocation with latency/errors/model version and redacted identifiers; no raw history logging. |
| `base.py`: `validate_input`, `process` | Abstract `NotImplementedError` hooks retired in favor of typed concrete entry points. | Six-definition worker inputs now have strict runtime JSON validation. Other family boundaries remain R1/R2/R3/R5/R6. |
| `config.py`: `AlgorithmSettings`, `get_algorithm_config` | Typed `RecommendationWeights`, `ModerationPolicy`, `DiversityPolicy`, `LensWeight` replace part of global settings. uv replaces Poetry/dependency lists. | Worker provider/executable/directory/timeout configuration and transport limits are wired. Configurable semantic models and legacy cache-policy parity remain open. BART model names in old config were never loaded by old content code. R4/R6. |
| `interface.py`: `__init__`, `_initialize_content_store` | Retired global network initialization and fake test-content seeding; old `add_content` call did not match its implementation. | Do not recreate seed behavior. R1 obtains real published Objects through core-owned interfaces. |
| `interface.py`: `process_content` | The served API's ingestion provider now invokes Python content/moderation plus spam/evidence analysis. | Verified publication/readback; policy enforcement and broader source agreement remain R3. |
| `interface.py`: `get_recommendations` | `RecommendationEngine.recommend` exists; old wrapper passed incomplete inputs and iterated an envelope as recommendations. | R1/R2/R5 supply profiles/candidates with privacy constraints and actual transport. |
| `interface.py`: `record_feedback` | `RecommendationWeights.with_feedback` now works in-process. Old wrapper called absent `update_engagement`. | R5 connect authenticated local events and persist per-user weights; no global success response without adaptation. |
| `interface.py`: `get_algorithm_status` | Startup health validates actual worker protocol, provider version and all seven supported definitions. | Detailed invocation latency/error operational reporting remains R6; startup success is not continuous health proof. |
| `main.py`: `ContentRequest`, `ContentResponse`, `FeedbackRequest`, `FeedbackResponse`, `RecommendationRequest`; `health_check`, `process_content`, `record_feedback`, `get_recommendations`, `get_related_content`, `get_status`, `general_exception_handler` | Retired broken FastAPI contract. Several routes awaited synchronous methods or invoked absent `record_user_feedback`/`get_related_content`; output shapes disagreed with models. | No Python HTTP replacement or related-content query exists. R1 defines core-owned validated contracts and real error mapping; R2 supplies related graph/semantic retrieval; R6 health. |
| `run.py` (no function declarations) | Retired dotenv/uvicorn `algorithms.main:app` launcher. | The Rust API now starts the real configured isolated Python worker; there is no replacement public Python HTTP server. Other family consumers remain open. |
| `models/recommendation.py` (no methods) | Duplicate legacy `RecommendationScore` model replaced by the sole recommendation result dataclass. | `score`/`final_score`, metadata, timestamp and Candidate fields differ. R1 schema mapping is mandatory. |
| `__init__.py`, `models/__init__.py` (no functions) | Current `babble_algorithms.__init__` exports actual implementations, including typed feedback and weights. | ContentAnalyzer/ModerationPolicy/LensWeight remain available from their modules. No alias imports of deprecated code. |

There is no production `feedback_loop_optimization.py` in the deprecated tree.
Legacy feedback-loop settings and recommender adjustment were audited above;
claiming a separate migrated optimizer would invent a missing implementation.

## Spec and Runtime Integration Matrix

The R1 execution boundary below is now implemented for core Judgments. R4 semantic
behavior remains open even when a transport and its seven lexical outputs pass.

| Requirement | Python implementation / limitation | Current runtime evidence and exact missing integration |
| --- | --- | --- |
| Typed uv algorithms; core owns durable records (spec line 36) | Python >=3.11, uv lock, strict basedpyright, ruff, typed models, `py.typed`, strict bounded worker JSON. | **R1 core Judgment slice verified:** default API Python provider, Rust-owned records and schemas, deadline/fault handling, publication preflight and live integration trace. Other family consumers remain open. |
| Versioned provider contracts, cache, uncertainty (spec 106-118, 217) | Six wire definitions mapped from existing algorithms. Relationship markers still do not evaluate target entailment. | Rust binds provider/model/version, definition, input, parameters, and output. Real-worker cache/fixture tests plus repeated API evaluations pass. **R4:** uncertainty remains explicitly heuristic or uncalibrated, not a trained probability estimate. |
| Real semantic inference and provider fallback | No Python Jev adapter, trained local model, model weights, embeddings, entity extraction, or semantic calibration. | The served node defaults to Python lexical rules; the native CLI retains Rust local rules. `judgment-jev` has Reqwest transport and orchestration has privacy routing, but neither proves live trained-model usage. **R4:** configure/evaluate a real semantic provider with model provenance, minimization, and verified outage behavior. |
| All named Judgment definitions (spec 110) | Seven wire definitions: spam, evidence_quality, relevance, relationship, content_analysis, moderation, source_agreement. | **R4:** topic, claim_type, novelty, constructiveness, ragebait, context_missing, information_density, semantic_distance still need their specified versioned schemas, meaning, model behavior and evaluation fixtures. Do not substitute marker counts and claim completeness. |
| Candidate safety -> graph -> semantic -> temporal/emerging -> exploration -> dedup -> bounded pool (spec 132-136) | Legacy `CandidateEngine.candidates` remains a supplied-ID utility, not the serving retrieval implementation. | Serving Rust retrieves from lexical, graph, recency, public-activity and deterministic sampling indexes, filters effective moderation restrictions before slots, and admits at most 200 before Judgment/temporal enrichment. Novelty retains corpus-wide author statistics. Emerging and exploration formulas refine provenance on that admitted pool. See [retrieval scope and costs](public-ranking.md). **R2:** global retrieval quality, semantic embeddings, safety policy evaluation and deployment scaling remain open; graph neighbors are not embeddings. |
| Lens composition, distinct experiences, trace (spec 140-146) | `ranking.py`, `ranking_types.py` and `ranking_wire.py` implement the canonical eight public Lens rules, weighted blends, exact DTOs and numeric traces. Legacy private-user Lens vocabulary is documented separately. | Default API uses the shared Python worker, validates outputs, returns provider provenance, and never silently substitutes native scoring. Nineteen Rust golden cases cover order and traces, plus a 200-candidate/all-lens test. Private chronological Following bypasses public ranking. Third-party evaluators and permissions remain separate work. |
| Post-Lens source diversity and saturation (spec 152) | Canonical Python ranking applies the same soft source-floor/share policy as the native reference. Legacy `FeedDiversifier` has additional creator/topic/seen concentration behavior and is not this public stage. | The SDK's `personalizeFeed` reapplies the canonical policy after private filtering/scoring and before the final limit. Primary-source accounting matches the native policy; secondary provenance is retained without double-counting floors. Nineteen native fixtures and client tests verify final traces, filtering and refill. **R2:** floors remain soft; admitted-pool coverage and empirical policy quality remain open. See [current live verification](public-ranking.md). |
| Epistemic state, provenance, minority positions (spec 120-128) | Source agreement now has a signed-public-evidence consumer, exact-copy suppression, atomic request/result history, and independently displayed components. Evidence/reputation vectors remain separate; the legacy scalar is not multidimensional truth. | **R3:** stronger source independence, semantic disagreement, federation, retention, and calibrated evaluation remain open. Signed provenance does not authenticate claims. Never replace hashgraph consensus with this module. |
| Local private personalization (spec 150-158, 371-379) | The Python library accepts caller-provided interests/expertise/history/peers; it does not itself own browser persistence, hidden/muted preferences, expertise estimation, encrypted sync or a local trained model. | `frontend/src/app/protocol.ts` invokes SDK `personalizeFeed`, which composes `personalizeCandidates` with final diversity. Browser settings and per-account local state already exist; Rust has a separate lexical `LocalPersonalizer`, and encrypted sync is a separate contract. **R5:** connect Python only in a deliberate local/private execution environment, or keep private ranking client-owned and feed Python public inputs. Do not send peer/user raw histories to a server for convenience. Complete feedback adaptation and replay protection beyond existing preference/history persistence. |
| Engagement, trends, temporal/emerging dynamics | `TemporalScorer` now runs through the worker; all components drive or explain public discovery. `EngagementAnalyzer` remains a supplied-event utility. | `node/src/discovery.rs` supplies publication time, quality and actual signed inbound public activity with a seven-day window; the old separate temporal formula/frozen clock is removed. **R6:** opt-in event/session collection, validation, retention, aggregation, empirical temporal evaluation, and invocation instrumentation remain open. No raw-private-history analytics upload is implied. |
| Semantic quality/evaluation/simulation (spec 281, 314, 333) | Python behavior tests only; no model evaluation harness, simulation engine, training loop or historical holdout corpus. | Rust `eval` and `sim` crates exist; `eval/tests/calibration.rs` uses local rules and a mirrored Jev test transport, not proof of independent live-model accuracy. **R7:** add calibrated labeled corpora, negation/paraphrase/abuse/context/privacy cases and historical replay with real provider artifacts. |
| Canonical fixture conformance | Canonical encoding plus Judgment worker and public ranking fixtures now span both languages. Ranking checks include ties, nanosecond timestamps, overflow-safe normalization, malformed outputs and exact echoed inputs with correctly rounded JSON float parsing. | **R8:** core schemas/encoders still own durable IDs/signatures. Fixture parity is not semantic quality, load readiness or exhaustive numeric proof. |

## September 30 Reconciliation

A fresh source review after the private social-safety milestone confirms the
remaining algorithm work is primarily capability and actual consumption, not
another worker transport. Earlier six-definition counts describe the original
worker milestone; `wire.py:DEFINITIONS` currently contains seven, including
source agreement. `JudgmentConfig::from_env` wires provider, executable,
algorithm directory and timeout. Bounded worker frames and batch contracts are
implemented. Legacy configurable semantic models and cache-policy parity remain
distinct open questions; the blanket claim that worker controls are unwired is
no longer accurate.

The publication path now calls `store.commit_publication(batch)` before changing
in-memory Object/edge/event state. The store owns a durable redo journal and
recovery. Current affected-crate regression evidence is recorded in the
[social-safety milestone](social-safety.md), not a fresh semantic evaluation.

Next substantial workstreams, preserving the original scope:

1. **Semantic Judgments and evidence interpretation.** Configure and evaluate an
   actual semantic provider through the existing versioned boundary. Relationship
   decisions must use both source and target meaning; relevance and evidence
   quality cannot be declared semantic merely because marker scores execute.
   Complete the eight outstanding named definitions in R4. Require independent
   labeled paraphrase, negation, contradiction, multilingual and misleading-citation
   evaluations, model provenance and honest outage behavior. The Jev calibration
   fixture currently delegates to `LocalProvider`; it is not independent model
   quality evidence.
2. **Candidate retrieval and final-feed diversity.** Indexed retrieval now precedes
   expensive enrichment, retaining lexical, graph, recency, public activity and
   deterministic exploration sources. Browser personalization reapplies canonical
   soft diversity before its nine-card limit. Integration evidence is tracked in
   [public ranking](public-ranking.md). Remaining work includes genuine semantic
   retrieval, empirical recall and policy evaluation, and costs of common lexical
   queries or high-degree graphs. Bounded provider work does not mean every
   retrieval operation is constant-time or globally optimal.
3. **Private recommendation and engagement lifecycle.** Connect the Python
   recommendation/engagement behavior to a real user-controlled execution path
   and feed consumer. Add validated session/event collection, deduplication,
   per-account model persistence and exactly-once feedback adaptation, without
   uploading raw private histories by default. Engagement currently counts each
   supplied event as a session; repeated events need real sessionization. Keep
   profile/expertise, semantic relevance and collaborative recommendations in
   scope rather than declaring heuristic weight persistence the finished system.

This reconciliation is a read-only source audit, not new model or runtime
verification. Public ranking, temporal scoring, local SDK personalization and
encrypted sync already exist and should be integrated rather than replaced.
Deprecated constant embeddings were placeholders; restoring those constants
would not satisfy the requested full implementation.

## Changes in This Audit

1. Added typed explicit-feedback adaptation, used by the existing recommendation engine.
2. Rejected invalid recommendation/Lens weights and normalized huge finite weights without overflow.
3. Made non-finite unit scores zero and normalized Lens candidates before mixing signals.
4. Excluded future/non-finite-timestamp history from historical recommendations, bounded
   interaction scores and supplied fallback recency.
5. Preserved observed discovery signals instead of replacing them with source priors;
   removed invented exploration novelty/weirdness and repeated whole-pool counting.
6. Prevented singleton self-agreement and duplicate-source inflation; empty-source
   evaluations now honor revocation of previously established consensus.
7. Restored Unicode word preservation with NFC normalization in the shared tokenizer.
8. Corrected README claims about semantic intelligence, simulation and runtime usage.

No placeholder transport, model, global user store, invented reputation authority,
or copy of backend auth was added. One existing Lens fixture now explicitly supplies
the relevance/novelty values it previously obtained through discovery overwrites.

## Initial Audit Verification and Limits

The evidence below is the historical pre-worker audit. The verified-worker
section above and `algorithm-worker.md` record the subsequent integration; retain
both milestones rather than interpreting the original 44-test result as current.

From `algorithms/`: `uv run --frozen pytest -q` passes **44 tests** (13 preexisting,
31 new); `uv run --frozen basedpyright` reports **0 errors, warnings, notes**;
`uv run --frozen ruff check` passes. Before implementation, the focused existing-API
regressions produced **16 failures / 1 pass**, including NaN -> 1.0 and singleton
consensus -> established.

Fozzy engine: `/Users/deepsaint/.cargo/bin/fozzy`, version 0.1.0. All test artifacts
use `/tmp` so parallel agents' repository artifacts are untouched. Final scenario:
`/tmp/babble-algorithm-migration-290929-checks.fozzy.json`; host runner:
`/tmp/babble-algorithm-migration-290929-checks.py`; seed **290929**.

- Strict `doctor --deep --scenario ... --runs 5 --seed 290929 --json` and
  `test --det --strict ... --proc-backend host --json` pass.
- `run --det --proc-backend host --fs-backend host --http-backend host --record
  /tmp/babble-algorithm-migration-290929-verified.fozzy --json` ran the actual uv
  environment, full pytest suite, basedpyright and ruff. Run ID:
  `062c9623-bc46-4e88-9929-fb0e7d75b6a4`.
- The host runner emits stable success lines only after child exit code zero and
  forwards full child output on failure. This avoids comparing pytest elapsed time
  in `proc_when` stdout contracts. An initial attempt failed that stdout comparison
  despite pytest passing; it was corrected, not waived with unsafe mode.
- `trace verify ... --strict`, `replay ...`, `ci ...` all pass for that host trace;
  checksum/version, replay outcome, warning parity and artifact checks are clean.
- `env`, `usage`, `report show`, `artifacts ls`, and scenario property `fuzz` (four
  runs) were inspected/executed. Doctor/fuzz use scenario contracts and completed
  too quickly to represent five/four real Python runs: they are **not** independent
  Python fuzz or semantic-quality evidence. Actual execution evidence is the
  multi-second host `test` and recorded host `run`, plus direct uv results.
- Distributed schedule `explore` does not exercise these stateless synchronous
  Python functions. No Python failure remained to shrink after regression fixes;
  the transient harness stdout mismatch is not a shipped algorithm failure.

No browser work was required. Rust, frontend, and SDK tests were not run by this
owner; runtime mapping above is source inspection, not an end-to-end Python
integration result. R1-R8 and the explicit partial/missing behaviors remain real
release blockers for an "all concepts fully migrated and integrated" claim.
