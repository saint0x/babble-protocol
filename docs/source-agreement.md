# Source Agreement

`babble.judgment.source_agreement.v1` connects the migrated Python consensus
algorithm to on-demand Object evaluations. This is source comparison, not
hashgraph finality, verified source independence, semantic entailment, or truth.
Its components and limitations remain separately inspectable. It does not affect
feed ranking or turn public reactions into votes.

## Runtime Path

The default Python provider advertises seven definitions. Four continue to run
during ingestion; source agreement runs only through the existing authenticated
Judgment evaluation REST/RPC workflow. The explicit Rust-local provider advertises
its six implemented definitions and rejects this seventh one without synthesizing
a result. No new Python HTTP service or Go path is involved.

The host builds the input, not the caller:

- Select incoming Supports, Contradicts, EvidenceFor, EvidenceAgainst, References,
  and Cites edges with HumanAssertion or ApplicationAssertion provenance.
- Verify edge and source signatures against historical signing identities.
  Exclude self-links, future records, edges before the claim, and sources published
  after their linking edge. Derived edges cannot recursively endorse themselves.
- Read only string payload fields `text`, `title`, `description`, and `summary`.
  No arbitrary-payload fallback is used for the claim or sources. Supporting
  moderation/evidence checks receive that same text-only projection, not raw
  metadata, account information, or session data.
- Collapse repeated edges by source ID and exact copied text by content hash.
  Empty source text contributes nothing. References/Cites are context unless a
  selected evidence relation for the same Object or an identical copy also exists.
- Reject more than 200 sources, any source over 64 KiB UTF-8, or more than
  512 KiB total source text before invoking supporting assessments. There is no
  silent truncation. Retrieval still scans local graph/storage collections.
- Obtain quality/evidence components from the configured provider. All selected
  public Objects use the unverified `social_media` source prior; metadata cannot
  promote itself to an official source. `user_id` and `vote` are null.

Only empty evaluation parameters are accepted. Reference time and previous score
come from the host and validated local evaluation history. A caller cannot submit
its own source array, previous score, clock, or source weights.

## Algorithm Meaning

The typed, bounded contract requires finite unit scores and valid timestamps,
unique nonblank source IDs, known source kinds, and no future source. Unknown
wire fields, anonymous votes, and missing required nullable fields are rejected.
Pure-library anonymous votes are accepted as data but excluded from scoring;
the worker boundary is deliberately stricter.

Term agreement is pairwise top-term Jaccard. The legacy fact-agreement field now
contains continuous vocabulary overlap from indicator-bearing sentences.
Negation is retained, but opposite claims can still overlap highly. This does
not establish that a source supports or contradicts the claim. Those signed
graph relations retain their separate meaning.

Reliability combines unverified kind priors and lexical quality/evidence
components. Age weight uses the oldest source and a seven-day half-life. Optional
named-user votes average within each user, then equally across users; contribution
values also average repeated users instead of overwriting them. The served path
supplies no votes. A neutral vote prior is not evidence of public agreement.

The legacy weighted composite and six-state transition remain in inspection data.
They are not calibrated probabilities. The UI shows independent lexical, prior,
and age components with an explicit limitation, or a real no-sources state.
It does not present the composite as a universal truth score.

## Persistence And History

An `ObjectJudgmentInput` binds the Object ID, Judgment ID, and exact provider-scoped
request. Publication atomically stages that association with its Judgment and
validates the input hash, definition, provider/output commitment, and result
binding. Reads validate the pair again. Historical records without an association
remain readable but do not receive invented inputs.

Unchanged source inputs reuse a persisted evaluation for less than 300 seconds,
including across restart. This preserves ID and creation time. Changed sources
or an expired evaluation create another record using the latest same-provider
score as previous state. Model/version changes do not reuse prior-provider state.
The current Object list merges associated on-demand results with legacy ingestion
results. The UI's Evaluation inputs disclosure fetches the exact stored request.
Late reads or evaluations cannot replace a newer inspector or reopen a closed one.

Supporting quality/evidence assessments are not separately committed by this
operation; their numeric results are retained in the agreement request. Inputs
whose privacy policy redacts the subject cannot claim an Object association and
retain the legacy Judgment-only path. This is not complete remote-provider history.
Re-evaluation compares the eligible public sources with the latest persisted
request before starting supporting workers. Unchanged requests inside the
five-minute window reuse the original state without repeating those assessments.
History lookup uses a derived SQLite index; the selected canonical input/Judgment
pair is still validated. Store opening rebuilds this index from canonical pairs,
and publication/recovery update it under the publication lock. See the store's
publication notes for recovery and out-of-band tamper limitations.

Source selection, supporting assessments, and aggregate evaluation share a
15-second computation budget. The Python transport uses the earlier of that
deadline and its configured per-call timeout. Deadline failure does not publish
partial results. Native synchronous provider checks are cooperative; lock waits,
filesystem calls, and process cleanup are not hard real-time operations. This
is not an HTTP admission/queue deadline. Reuse still examines the graph, and
batching of supporting assessments remains future optimization work.

## Evidence And Remaining Work

Focused API tests exercise real Python, signed evidence selection, limits,
privacy observation of worker frames, HTTP/RPC readback, restart, changed history,
forged parameters, and malformed-output persistence protection. Store tests cover
atomic recovery, corruption, immutable retries and request/output commitments.
Frontend tests cover literal rendering, lazy loading, retry, missing history,
empty evidence, independent metrics, and stale inspector requests.

The actual API/Astro/Aegis suite publishes two signed evidence relations, evaluates
through the form, checks all four components and the limitation, expands exact
persisted inputs, and repeats without losing the old evaluation or creating a
duplicate. Its trace `artifacts/source-agreement-live-history-host.fozzy` passes
strict verification, replay, and CI. The focused API trace is
`artifacts/source-agreement-api-verified-host.fozzy` with the same checks.
Store's final confidence-binding and crash-recovery trace is
`backend/crates/store/artifacts/object-judgments/final-confidence-host.fozzy`.

Full Rust workspace tests, 732 Python tests with basedpyright/Ruff, 256 frontend
tests, 65 SDK tests, canonical fixture conformance, and Astro check/build pass.
The final store-only confidence binding was additionally verified by all 41 store
tests. Scripted Fozzy determinism/fuzz checks cover orchestration, not domain input
fuzzing or proof that live external processes are deterministic.

This closes a runtime consumer gap, not the entire algorithm migration. Source
independence/Sybil resistance, calibrated semantics, operator retention,
federation, privacy review, and the remaining named definitions are still
open in [production readiness](production-readiness.md) and the
[migration matrix](algorithm-migration.md).
