# Private Account Safety Backend Handoff

Source freeze: September 30, 2026, verified after final host regression.
All 27 source/export hashes in `artifacts/safety/source.sha256` match.
No frontend or SDK sources, browser tools, or the live node data store were touched.

## Contract

- Authenticated host-only `GET /social/safety`, `GET /social/safety/{id}`, and `PUT /social/safety/{id}`.
- PUT body: `{blocked, muted, expected_revision, idempotency_key}`.
- `SafetyState = {author_id, target_id, blocked, muted, revision}`.
- `SafetySnapshot = {author_id, revision, entries: [{identity, state}]}`.
- Untouched pair revision is zero. Active targets are sorted by identity and bounded at 1,000.
- Unblocking preserves the explicitly supplied mute state. Clearing works at capacity.
- Pair tombstones and exact-request receipts persist. An old retry returns its original result without reapplying it.
- Snapshot revision changes only for state changes. Following cursors include that revision.
- Responses, including authentication failures, are no-store. Embedded document requests cannot use these routes.
- Block rejection uses the generic conflict message `interaction unavailable`.

Canonical schema names: `node.SafetyState`, `node.SafetySnapshot`,
`node.SafetyEntry`, `api.SetSafetyRequest`. Signed fixture schemas:
`graph.SafetyAction`, `graph.SafetyReceipt`.
The deterministic fixture key is `social_safety`.

Both exports are current. Regenerate from the repository root:

```sh
CARGO_INCREMENTAL=0 cargo run --manifest-path backend/Cargo.toml -p babble-schema --bin export -- bundle fixtures/protocol/v1/schema-bundle.json
CARGO_INCREMENTAL=0 cargo run --manifest-path backend/Cargo.toml -p babble-schema --bin export -- fixtures fixtures/protocol/v1/fixtures.json
```

## Implementation

- `backend/crates/graph/src/safety.rs`: canonical state, snapshots, domain-separated signed actions and receipt chains.
- `backend/crates/store/src/safety.rs`: private SQLite CAS transactions, exact-request receipt verification, capacity, snapshots, and startup integrity verification. Storage is `private_safety/safety.sqlite3`, with directory 0700 and database 0600. Initial installation is atomic; missing established storage fails closed.
- `backend/crates/node/src/safety.rs`: account reads/writes and shared interaction checks.
- `backend/crates/node/src/publication.rs`: guards generic Objects, embedded edges, provenance, related publications, and direct edge publication, after exact publication-receipt replay.
- `backend/crates/node/src/lib.rs`: startup verification and separate inferred-edge guard.
- `backend/crates/node/src/invocations.rs`: live bidirectional enforcement for  follow/reply/share execution.
- `backend/crates/node/src/following.rs`: follow enforcement, owner-specific feed filtering, and safety snapshot cursor invalidation.
- `backend/crates/node/src/reactions.rs`: prohibits new reaction intent under blocks while allowing complete or partial withdrawals and exact retries.
- `backend/crates/api/src/safety.rs`, route registration, and auth policy: authenticated transport and no-store behavior.
- `backend/crates/schema/src/safety.rs` and schema registration: reproducible signed fixtures and canonical exports.

The custom `unfollows` exemption is restricted to canonical HumanAssertion edges,
exact `removes_relation: "follows"` metadata, the actor's signature, and an
existing signed follow for the same source and target. The source must be owned
by the actor, or bound to the currently approved one-use unfollow invocation.
That latter case preserves legitimate withdrawals through application-owned
Surface controllers. A label alone, extra metadata, missing history, wrong
origin, or an arbitrary unowned source cannot bypass a block.

Public imports, public Object/profile/graph reading, public discovery and
Judgment inputs retain their public semantics. Private records never enter
public Objects, graph edges, events, or export bundles. Host discovery filtering
is the parent's frontend responsibility; Following filtering is enforced here.

## Verification

All commands used `CARGO_INCREMENTAL=0` for Cargo and the real
`/Users/deepsaint/.cargo/bin/fozzy`.

- 26 focused safety tests passed: graph 3, store 10, node 8, API 4, schema 1.
- 495 affected-crate tests passed across 42 test binaries, covering all tests in graph/store/node/API/schema.
- Strict doctor audit: five runs, seed 9302026, consistent.
- Strict deterministic test with host process/filesystem/HTTP backends passed.
- Final recorded host regression passed, followed by strict trace verification, replay, and CI, all passing without warnings.
- Final source/export checksum verification passed.

Primary evidence under `backend/crates/node/tests/artifacts/safety/`:

- `final-regression-host.fozzy`: final, authoritative real host trace.
- `native.log.qR3fjy`: complete 495-test regression output.
- `native.log.wNF6uA`: final 26-test focused safety output.
- `source.sha256`: exact source/export manifest.
- `doctor.json`: five-run preflight audit.
- `strict-host-tests.json`: focused strict host test report.

Final host run: `e346c46d-5d91-43e3-9da1-d92caa3dd9c5`.
Final replay: `ceb4650a-d760-4b10-b470-bf4b831b0139`.
Focused strict host run: `5eec9efd-909c-4bad-9296-d10a27a467e8`.

Fozzy's doctor internally uses scripted process preflight. The scenario's
`proc_when` is a preflight declaration and a host execution assertion, not the
feature evidence. The strict host test and final recorded `proc_spawn` actually
execute Cargo tests; their retained logs are the backend evidence. Auxiliary
fuzz output is not counted as native feature coverage. Earlier intermediate
traces are superseded by `final-regression-host.fozzy`.

Re-run final backend verification from the repository root:

```sh
/Users/deepsaint/.cargo/bin/fozzy doctor --deep --scenario backend/crates/node/tests/safety-host.fozzy.json --runs 5 --seed 9302026 --json
/Users/deepsaint/.cargo/bin/fozzy test --det --strict backend/crates/node/tests/safety-host.fozzy.json --proc-backend host --fs-backend host --http-backend host --json
/Users/deepsaint/.cargo/bin/fozzy run backend/crates/node/tests/safety-regression-host.fozzy.json --det --seed 9302026 --proc-backend host --fs-backend host --http-backend host --record backend/crates/node/tests/artifacts/safety/final-regression-host.fozzy --json
```

## Remaining Work

No known backend implementation gaps remain in the requested contract.
Parent owns live API/frontend/browser acceptance, account-switch and
late-response behavior in the host, and SDK integration. Those browser flows
were deliberately not tested here. Reporting/moderation/appeals remain outside
this contract.
