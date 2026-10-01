# Semantic Integration

Source audit, September 30, 2026. This is an implementation map, not a claim of
model capability. The semantic service's actual API contract, endpoint, model
revision and authentication configuration still need to be identified.

## Current Runtime

`JudgmentConfig` in `backend/crates/api/src/provider.rs` accepts only `python` and
`rust-local`. The default Python worker reports `babble-python/lexical-v1/1`.
`babble-judgment-jev` defines a custom HTTP request/response, but the serving API
does not select it. Its scripted test transports are not a live Jev service.
Do not assume that this custom DTO matches an actual vendor API.

The active registry and Python worker implement seven definitions. The separately
versioned `topic`, `claim_type`, `novelty`, `constructiveness`, `ragebait`,
`context_missing`, `information_density` and `semantic_distance` definitions are
absent. Content-analysis topic keywords do not implement `topic.v1`.

The following source findings determine integration work:

- `relationship_judgment_state` constructs source and target text separately.
  The audit found that the local Python privacy policy dropped both, leaving
  only concatenated `text`. The current repair preserves these explicit public
  fields; a real-worker/orchestrator regression verifies source-only marker
  interpretation, binding of both texts and continued exclusion of private
  history. Remote policy is unchanged. Python still explicitly reports that
  target context was not evaluated; this is not semantic entailment.
- Evidence quality counts lexical markers, relevance measures token overlap,
  and source agreement measures vocabulary overlap. Agreement has no target
  claim text. Contradictory propositions can have high lexical overlap.
- Worker health, output validation and schemas pin lexical provenance and the
  current definition list. Adding registry entries alone would break health
  compatibility, not create executable capabilities.
- Publication currently requires four ingestion Judgments before commit.
  Replacing the provider wholesale with a remote dependency would make ordinary
  publication unavailable during model outages.

## Required Boundaries

Rust continues to own schemas, canonical IDs, signatures, Judgment commitments,
cache keys and durable associations. Typed Python/uv owns definition-specific
procedures, model-output interpretation and deterministic aggregation. Vendor
types stay inside a narrow adapter. Public ranking and temporal algorithms remain
independent of semantic inference.

A local worker calling a remote model is a remote privacy route. Its capability
manifest must identify actually executable definitions and resolved model versions.
Credentials need explicit minimal injection through the current `env_clear`
boundary, not ambient inheritance or frontend configuration.

Definition-specific minimization must cover nested fields and parameters, not
only top-level `text`. Pair comparisons need balanced source/target budgets and
observable truncation. Private intent, histories and personal novelty references
remain local unless separately and explicitly authorized.

Host-side Object association must survive provider-visible subject redaction.
Currently `prepare_judgment_request_before` records the input association only
when the minimized subject still equals the Object ID. Actual provider/model,
definition, procedure/calibration version, minimized inputs, material parameters,
reference snapshot, taxonomy and rubric must bind caches and commitments.

Optional semantic enrichment needs explicit unavailable/pending states during
outages, without fabricated zeros or lexical results wearing semantic provenance.
Cascade rejection and insufficient-confidence states must be distinguishable from
accepted results. Human-reviewed moderation remains separate from model signals.

## Definition Inputs

These are requirements for the next canonical contract, not newly available APIs.
Incompatible changes to existing meanings or outputs need new definition versions.

| Definition | Inputs and output meaning |
| --- | --- |
| Topic | Text/language and versioned taxonomy; scored topic IDs, supporting spans and unknown cases. |
| Claim type | Text and bounded discourse; classified claim spans, not one label for every sentence. |
| Novelty | Subject, bounded reference set, time and retrieval coverage; scoped new/repeated propositions, never global novelty. |
| Constructiveness | Contribution, target discussion and rubric; reasoning, clarification and actionable contribution components. |
| Ragebait | Text and relevant context; grounded provocation signals that distinguish quotation, criticism and disagreement. |
| Context missing | Claim, task and supplied context; missing categories without invented supporting facts. |
| Information density | Text/language and segmentation version; distinct propositions, redundancy and explicit denominator. |
| Semantic distance | Two texts and metric scope; topical/propositional distance, distinct from contradiction. |
| Relationship | Separately bound source/target and requested relation; support, contradiction or relatedness with grounding and abstention. |
| Evidence quality | Target claim, excerpts and supplied provenance; relevance/directness/methodology and explicit citation-verification status. |
| Relevance | Candidate and permitted scoped intent/query; graded relevance without sending complete private profiles. |
| Source agreement | Target claim, bounded sources/provenance and time; per-source stance, dependency groups, disagreement and transparent aggregation. |

Validate finite ranges, span bounds, taxonomy membership, referenced sources and
cross-field invariants in both Python and Rust. Scores, calibrated uncertainty,
insufficient input and provider failure are different outcomes. A model's claimed
certainty is not calibration evidence.

## Verification Gate

The existing `DeterministicJevTransport` in the calibration tests delegates to
`LocalProvider`; agreement with it is not independent model-quality evidence.
Build separately adjudicated, versioned holdouts for each definition, distinct
from prompt development and calibration data. Cover target swaps, paraphrases,
negation, contradictory claims, misleading citations, multilingual content,
quotation, missing context, dependent sources and prompt injection.

Predeclare thresholds for task quality, calibration/abstention coverage and subgroup
regressions. Record actual model/procedure revisions, inputs, outputs, latency and
cost. Constrain context, batching, concurrency, retries and expenditure. A replay
proves reproducibility of recorded behavior, not new inference quality.

Run strict Fozzy doctor/test first, then real host execution, trace verification,
replay and CI. Mock transport checks and canonical fixture parity remain useful
contract tests, but must not be reported as operational semantic-model proof.
