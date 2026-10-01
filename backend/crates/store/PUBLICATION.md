# Atomic publication

`PublicationBatch` adds a bounded redo journal without changing existing record
paths, JSON formats, IDs, event payloads, or signatures. Node text, record, draft,
media, reply, share, fork and remix publication commits the new Object, its signed
event, all ingestion Judgments, and any associated edges and signed edge events
as one batch. Standalone edge publication and ingestion refresh also use batches.
Provider evaluation and protocol validation finish before any commit work.

## Commit and recovery

1. Acquire the store's OS file lock. Reject handles marked recovery-required or
   stores with an outstanding committed journal. Check all conflicts and limits.
2. Write and sync `.publication-prepared`, containing the complete records and
   prior Judgment values, a format version, and a digest of the exact payload.
3. Rename it to `.publication-committed`, then sync the store directory. This is
   the durable commit decision. No canonical record changes before the rename.
4. Write and sync each record's temporary file, rename it into the legacy path,
   and sync every affected collection directory.
5. Update the derived Object Judgment SQLite index in one `synchronous=FULL`
   transaction, still holding the publication lock. An index error retains the
   committed journal and invalidates the writer for recovery.
6. Remove the committed journal and sync the store directory again. Only then
   does the node apply its prepared state and cache changes and return success.

Startup takes the same lock and recovers before loading records into the node.
It verifies journal bounds, version, digest, record types/IDs, signed records
against stored signing history, and every destination's old-or-new value before
installing anything. A committed batch is always rolled forward; recovery is
idempotent. Corruption or conflicting data prevents opening the store and retains
the journal for diagnosis. Uncommitted preparation is discarded. Never manually
delete a committed journal to bypass a recovery failure.

All FileStore identity/object/edge/event/judgment reads and writes share the lock.
Readers cannot observe a partially installed batch through those APIs. Separate
read calls are not a multi-query snapshot. Direct filesystem access and older
binaries that do not honor the lock are outside this contract. Active nodes must
not share a root as independent writers: their in-memory indexes are not a
cross-process coherent cache, even though file operations are serialized.

## Errors and limits

`PublicationError::Precommit` guarantees no canonical publication writes. Node
indexes and Judgment cache entries/hit counts stay unchanged. Object and edge
IDs must be new; existing events must match exactly. Judgments may refresh because
their IDs omit evaluation time. Duplicate batch entries are rejected.

`Uncertain` covers rename/commit-directory-sync failure; the decision may have
persisted. `Committed` covers failure after the decision was synced. Both include
the journal digest as transaction identifier and invalidate the failed handle
(including clones). Reopen the store/node, inspect recovered Objects/events, and
reconcile before issuing a fresh publication. The covered RPC operations below
can reuse their original idempotency key after reopening; other blind retries
can create another Object. The existing node `Result` API exposes these distinctions in the Conflict
message; it does not turn a committed I/O failure into success. A failed node keeps
its old in-memory publication snapshot until reopening; record APIs fail closed.
All fallible public node operations call `LocalNode::check_ready()` before work,
so even legacy mutations cannot change memory after a publication failure.
API dispatchers must also call that method under their node lock before using
infallible snapshot getters (`object`, `edge`, `identity`, `event`, graph/realtime
views). Those getters retain their existing signatures; reading them directly
without the readiness gate can return the old snapshot. `FileStore::check_ready()`
provides the equivalent gate for storage clients.

Each batch is at most 256 records and 16 MiB including its envelope and preimages.
Only batch records are staged, never the whole store. Each Object Judgment and
its input association consume two records; each provenance edge and its event
consume another two. Existing oversized destination
records also fail before commit. OS locks and file/directory sync support are
required; use a local filesystem with reliable atomic rename and fsync semantics.
Process-exit tests exercise software ordering, not physical power-loss durability
of a particular filesystem or storage device.

Blobs, identities, key files, capability grants, imports, realtime state, storage
capability values, and inferred-relationship operations remain outside publication
transactions. A blob uploaded before a rejected publication can remain unreferenced.
Legacy records are not retroactively repaired or given transaction histories.

## Object Judgment Inputs

`ObjectJudgmentInput { object_id, judgment_id, request }` retains the actual
provider-scoped `JudgmentRequest`, including subject, context and parameters.
`PublicationBatch::object_judgment(&input, &judgment)` validates and stages both
records or neither. Associations live in `object_judgment_inputs/<judgment_id>.json`
and use the same redo journal, lock, byte limit and recovery protocol as Judgments.
No separate association write API exists.

An association is immutable. A retry with identical association content is
accepted, but different content for an existing judgment ID fails before commit.
Duplicate IDs within a batch are rejected. Judgment evaluation time and confidence
may refresh, as before, provided the association still validates. The binding
checks Object ID validity, exact subject equality, registry request/output rules,
canonical state hash, matching definition and judgment ID, finite confidence in
`[0, 1]`, and the existing provider commitment over definition, provider version,
input hash, request parameters and output. Source-agreement results additionally
pass the core request/output binding validator. The caller supplies the actual scoped
request; the store cannot infer a provider's privacy policy or omitted context.

Recovery requires an association's Judgment in the same journal and validates
both new values and preimages before any installation. Existing association
preimages require Judgment preimages. Standalone Judgment writes also validate
any existing association so they cannot invalidate its binding. The legacy
`put_judgment` API now uses the same journal and index transaction, including
metadata-only refreshes. Its `Result` error preserves the publication error text.

`get_object_judgment_input(&JudgmentId)` returns the validated association or
`None` when absent. `object_judgment_inputs(&ObjectId)` returns associations sorted
by judgment ID. Both hold the publication lock while checking stored counterparts;
corrupt, misnamed, oversized or missing counterpart records produce errors.
History reads use `object-judgments.sqlite3` to select only the requested Object's
rows, then read and validate those canonical pairs. They do not scan unrelated
associations. `latest_object_judgment_input(&ObjectId, &DefinitionId,
&ProviderVersion, Timestamp)` returns `Result<Option<(ObjectJudgmentInput,
Judgment)>>`: the greatest `(created_at, JudgmentId)` with `created_at <= reference`
for the exact Object, definition and provider/model/version. A covering SQLite
range index selects at most one pair. Seconds and nanoseconds are separate integer
columns, preserving precision and ordering across time zones and dates before the
Unix epoch. A corrupt selected pair is an error, never a fallback to an older row.
Old Judgments without associations remain readable through the existing APIs.
These records preserve provenance, not proof that a provider actually evaluated
the supplied request, and do not require an Object record in the same batch.

### Derived Index Recovery and Trust

The association and Judgment JSON files remain authoritative. Every store open
holds the publication lock, rolls forward any committed canonical records, and
rebuilds the entire derived index by validating every association/counterpart.
This also migrates existing flat records and repairs missing, corrupt, obsolete,
incomplete or misindexed derived data. Bad canonical data prevents open; it is
never skipped. Only after a successful rebuild can recovery retire the journal.

Rebuild uses a separate SQLite database and transaction, closes and syncs it,
removes stale derived-database sidecars, atomically replaces the index, and syncs
the root directory. Rebuild interruptions are retried on open. Runtime updates
are all-or-none SQLite transactions after canonical installation and before
journal retirement; recovery is safe on either side of their commit. A process
exit inside SQLite may leave a hot rollback journal, which is discarded with the
old derived database during rebuild. No index connection survives the publication
lock, so independent FileStore handles and processes see replacements and updates
without an in-memory history cache. This does not change the separate node cache
restriction described above.

Queries never create a missing database or schema. Unknown application/schema
versions, missing readiness markers, SQLite read errors, missing selected files,
and discrepancies between selected row metadata and canonical data fail closed.
Reopen to rebuild. The index and its metadata are not authenticated storage:
arbitrary direct disk/SQLite edits while handles are open can delete rows, move
them outside a query, or otherwise hide data without being detected by that
query. Unselected canonical corruption is likewise detected on reopen or when
selected, not on every unrelated lookup. Proving completeness against a hostile
local operator would require a different integrity contract. Stop writers before
offline edits and reopen afterwards. Back up canonical JSON and any committed
journal together; the index can be rebuilt and is not a source of history.

Run the store scenario from this crate with the Fozzy engine:
`fozzy doctor --deep --scenario tests/object-judgments.fozzy.json --runs 5 --seed 930 --json`,
then `fozzy test --det --strict tests/object-judgments.fozzy.json --json`.
Use `fozzy run tests/object-judgments.fozzy.json --det --proc-backend host --fs-backend host --http-backend host --record artifacts/object-judgments/host.fozzy --json`
for actual Rust execution. The runner sets `CARGO_INCREMENTAL=0` and saves native
test output under `artifacts/object-judgments/`. Scripted runs alone only validate
the scenario contract. Crash tests cover every publication phase for initial
association creation and refresh, followed by reopen and idempotent retry.
The same suite covers migration from flat records, real SQLite transaction
rollback on failed index writes, both index crash boundaries, failed rebuild
journal retention, metadata refresh via legacy writes, independent handles,
nanosecond/tie ordering, selected-row corruption and misindexing. Read-boundary
instrumentation asserts the exact canonical paths touched by history/latest
queries, and `EXPLAIN QUERY PLAN` verifies a covering range search without a scan
or temporary sort. Artifacts named `indexed-history-host.fozzy` are the host-backed
evidence for the indexed implementation; trace replay checks the recorded run,
while the host run itself executes the Rust/SQLite/crash tests.

Indexed-history verification on 2026-09-30 used
`/Users/deepsaint/.cargo/bin/fozzy`, seed `930`: deep doctor (five runs), strict
deterministic test, host-backed recorded run, strict trace verification, replay
and CI all passed. Host run `f40513ef-8d8e-4f46-b15a-dbb020dc7541` executed all
50 store tests (32 unit, 18 integration) with `CARGO_INCREMENTAL=0`; native output
is `artifacts/object-judgments/store-tests.0qHg4e`. Eight scripted fuzz runs also
passed, and report/artifact inspection confirmed the host trace. `explore` does
not accept this process-steps scenario (it requires a distributed scenario);
actual crash scheduling is covered by the Rust process-exit tests. No failing
implementation trace required shrinking.

## Durable RPC Publication Retries

The ten Object/social publication RPCs (text, draft, media, fork, remix,
standalone edge, follow, unfollow, reply, share) commit a private retry receipt
in the same batch as their records. Keys must be nonempty, trimmed, and at most
256 bytes. Receipt IDs bind author, origin, bound Object, and caller key;
fingerprints additionally bind method, payload, and sorted unique grant IDs.
Runtime and Surface-session IDs are excluded so reconnecting clients can retry,
but HTTP ownership and all current runtime/grant authorization still run first.

The node serializes publication and receipt lookup under its existing exclusive
borrow/mutex. Replaying returns the original signed domain records without a new
publication or provider evaluation. Related-edge cardinality and semantic intent
are checked against fresh validated candidates; corrupt/missing records produce
errors, not synthetic successes or a panic. Recovery requires a receipt's Object,
event and complete edge set to be in the same journal. Receipts are private host
metadata, not signed protocol records and not protection against a compromised
store operator.

Receipts currently live as long as the publication; they are not silently expired.
Keep them in backups with the corresponding records. There is no retention/GC
policy yet. REST publication, import, inferred relationships, grants, identities,
storage, realtime, and other mutating RPCs do not gain durable retry deduplication
from this implementation. The native `with_publication_request` adapter is a
trusted, single-publication scope, not a multi-operation transaction API.

## Verification

Store tests cover 19 error/exit boundaries (including receipt installation), restart and repeated recovery,
concurrent readers, conflicts and bounds, Judgment replacement, and malformed,
corrupt, or conflicting journals. Node tests force real preparation/install I/O
failures through all nine publication entry paths, verifying unchanged live
indexes/cache before commit and complete signed social/provenance recovery after
commit. Existing publication, replies, grants, and local-node tests remain intact.

Run `bash tests/run-atomic-publication.sh` from the node crate. The dedicated
`tests/atomic-publication.fozzy.json` and `tests/atomic-store.fozzy.json` scenarios
support strict scripted determinism checks and host-backed execution. Scripted
passes alone do not execute Rust tests; host traces and native logs are the actual
implementation evidence, under `node/artifacts/atomic-publication/`.
