# HTTP Accounts and Authorization

The exported API router always authenticates protected requests. Native Rust
`dispatch_rpc_request` is a trusted in-process API, not an HTTP authorization API.

## Account Contract

| Request | JSON body | Successful response |
| --- | --- | --- |
| `POST /auth/register` | `{handle, kind: "Person", password}` | `200 {identity, token, expires_at}` |
| `POST /auth/login` | `{identity_id, password}` | `200 {identity, token, expires_at}` |
| `GET /auth/session` | None, bearer header | `200 {identity, expires_at}` |
| `DELETE /auth/session` | None, bearer header | `204` |
| `GET /auth/sessions` | None, bearer header | `200 {sessions: [{id, created_at, expires_at, current}]}` |
| `DELETE /auth/sessions/{id}` | None, bearer header | `204`, owner-scoped and idempotent |
| `POST /auth/sessions/revoke-others` | Empty, bearer header | `204`, keeps acting login |
| `POST /auth/password` | `{current_password, new_password}`, bearer header | `204`, signs out all logins |

New passwords contain at least 15 Unicode scalar values and at most 1024 UTF-8
bytes. Login still accepts existing credentials created under the former policy.
Password changes verify the current password, compare the verified hash again
inside the transaction, update the hash, and revoke every session atomically.
Wrong current passwords return 403 without revocation. New session issuance also
compares the hash it verified, closing the concurrent old-password login race.
No secret is trimmed, normalized, or silently truncated. Handles contain 1 through 128
UTF-8 bytes after trimming, without control characters. Existing IdentityKind
variants are accepted with their existing capitalization. Identity ID is the
canonical login key; handles need not be unique. Registration always creates a
new identity. Existing identities, including seed identities, cannot be claimed
by supplying their handle or ID. No password reset or recovery endpoint exists.

Tokens contain 32 cryptographically random bytes encoded as 64 hexadecimal
characters. Send them as `Authorization: Bearer <token>`. Each session expires
seven days after issuance; expiry is RFC3339 UTC. Logout revokes only that token,
durably. Login after logout or expiry issues a new token. At most 16 active
sessions are retained per account. Authentication responses use `Cache-Control:
no-store`. Invalid credentials, unknown accounts, revoked and expired sessions
return 401 with the same `unauthorized` error. Ownership failures return 403.

Management IDs are independent random `account_`-prefixed 64-hex strings, not
bearer tokens or token hashes. The atomic migration preserves existing tokens
and expiries, assigning unknown legacy creation times as null. Session lists
are private, contain at most 16 active rows, and expose no credentials. A foreign
or absent management ID returns the same 204 on deletion without affecting its
owner. Revoking the acting session requires signing in again. Password-change
retries after successful revocation return 401; a lost response is an uncertain
outcome, not proof of rollback. See [the client recovery contract](../docs/authentication.md).

Argon2id uses unique random salts, version 19, 19 MiB, two iterations, one lane.
Password work runs on blocking workers outside the node lock. Four simultaneous
password operations and 60 attempts per minute per API instance are permitted;
capacity exhaustion returns 429. This global admission limit bounds CPU and
memory; production ingress should additionally enforce distributed/per-client
limits and TLS. There is no trusted forwarded-IP authentication shortcut.

## Ownership

All protected HTTP/RPC operations require the authenticated principal, never a
caller-provided identity or `host` label. Unknown HTTP operations and newly added
RPC methods fail closed until explicitly classified in `auth/policy.rs`.

Public reads include discovery feeds, objects, graph traversal, media, catalogs,
and Surface preparation. Anonymous preparation uses no user grants. Authenticated
preparation and Surface start use only that principal's signed grant events.
Judgment evaluation, media upload, publication, social actions, private storage,
and runtime operations require authentication.

Person-following is host-only: `/social/following`, `/social/following/{id}`, and
`/feed/following` require a user session and return `Cache-Control: no-store`.
The principal defines the acting account. These records are separate from public
Object-follow edges, are not exported in event bundles, and are not exposed to
embedded Surfaces. See [the Following contract](../docs/following.md).

Capability grants represent viewer consent, independent of Object authorship.
The grant event author must equal the session principal. Every supplied grant ID
must belong to that principal and the bound Object. A user can revoke only their
own grants. Revoked/expired grants are rejected by the capability broker.
Grant projection uses one event-store read. Imported revocations by another
actor are ignored, including after restart. Allow-listed realtime room reads
also require membership; signing in does not make private room state public.

Object-bound RPC can invoke only explicitly allowed Surface methods and public
reads. Host publication, grant changes, identity administration, global operations,
and runtime lifecycle mutation are forbidden through Object-bound calls. The
client host must independently enforce this distinction before forwarding iframe
requests with its bearer token.

Host-owned social controllers use an Object binding with `surface_session_id:
null`. Synthetic Surface IDs are rejected, including for Object authors. Every
explicit Surface binding must match a real runtime session, its Object, identity,
and originating account login. Local storage
is partitioned by principal; Object storage remains shared among viewers whose
own grants authorize access. Runtime checkpoint keys are excluded from Object
storage RPC and are available only through the owning runtime session.

Surface ownership is durably bound to the originating bearer-token hash. Another
login for the same identity cannot inspect, execute, claim, or close that login's
Surface sessions. Starting an existing nonterminal, unexpired session with the same login,
Object, role and ID returns it. Failed starts remove new reservations. Native
sessions cannot be claimed over HTTP. Migration retains legacy ownership IDs
without inferring a login binding; these unbound rows fail closed. Create a fresh
Surface session after migration or account-session replacement.

Surface execution permits only `prefetched`, `warm`, and `active`; `cold`,
`suspended`, and `evicted` are denied. Object ownership is not an exception.
Host lifecycle management can resume a suspended session. The originating,
still-valid login can inspect an evicted session with REST/RPC session GET and
repeat eviction. Successful explicit eviction retires its ownership row, so that ID cannot
be restarted, including after a node restart. The native runtime itself remains
in memory: surviving nonterminal ownership can resume the same ID after restart
with its original login and durable checkpoint.

HTTP admission is rechecked after acquiring the node execution mutex: a queued
request cannot execute using revoked/expired credentials or an invalidated
Surface. Native dispatch and operator authorization retain their separate trust
boundary. This does not roll back a mutation already executing under that lock.

Logout revokes the token and drains only that login's pending Surfaces under the
node lock, using indexed batches of 128. A server task sweeps expired, revoked,
or unbound ownership without client cleanup. It scans at most 128 pending rows
per lock acquisition, advances past live rows, immediately continues full
batches, and waits one second between completed passes. Partial indexes exclude
retired rows. Runtime eviction precedes the ownership retirement write. A failed
write leaves the row pending; the sweep also retires already-evicted runtimes,
even while their originating login is valid. Cleanup retries failures; lock contention or storage failure can
delay retirement, while credential and lifecycle checks deny new execution.

Authenticated HTTP sessions also have a persisted 60-second host lease, renewed
only through the explicit host heartbeat. Expiry denies execution and renewal
immediately, and the bounded worker evicts abandoned sessions even when their
login remains valid. Reads and embedded Object calls cannot extend the lease.
Migration gives pre-lease rows deadline zero. See [the lease contract](../docs/surface-leases.md)
for endpoints, timing, restart behavior and verification. Terminal runtime records
and durable ownership tombstones are retained; garbage collection and saved-state
recovery remain separate work. Backend retirement does not itself stop a browser
iframe; the frontend watchdog and renewal-failure teardown enforce local exit.

## Operator and Storage

Moderation REST endpoints under `/moderation/` require an ordinary account session
and return `no-store`, including failures. Only explicitly configured canonical
identity IDs in `BABEL_MODERATOR_IDS` can review; empty configuration grants nobody
authority and malformed configuration prevents startup. Self-review and appeals
reviewed by the original decision maker are denied. Object-bound RPC and Surface
documents cannot use moderation endpoints. Private signed receipts, case history,
and current restrictions remain outside public events and exports. Restrictions
are checked at Surface preparation, execution, heartbeat, and bundle-gateway
authorization; public signed Object reads remain intact. See
`../docs/moderation.md` for workflow, redaction, and operator setup.

`BABEL_OPERATOR_TOKEN` is an optional environment-only credential of at least 32
bytes. Without a valid configured token, HTTP event export/import, consensus
checkpoint operations, global observability and runtime health are inaccessible.
User tokens cannot access these operations. Never expose this token in the UI.
Legacy HTTP `/identities` and RPC identity creation are disabled for everyone;
registration is canonical.

Accounts and token hashes live in `<BABEL_STORE_ROOT>/auth/accounts.sqlite3` using
SQLite transactions and full synchronization. Only hashes of passwords and bearer
tokens are stored. Signing keys, which the custodial node needs to publish on an
account's behalf, live separately under `signing_keys`, versioned by public key.
Directories are mode 0700; credential/key files are mode 0600. Unix permissions
are required for account storage. Keep the entire store private and back it up
as one unit. Account-store initialization failures prevent the server starting;
an exported router with a failed store still rejects protected operations.

Use one serving node per store root: the native event/object state and runtime
are in memory even though account transactions are durable. Horizontal serving
requires a shared node-state/ownership coordination layer. Account provisioning
and native identity publication are separate durable writes; an I/O failure
during provisioning can leave an unclaimable orphan identity, never an account
that can claim a pre-existing identity.

## Verification

Run `CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml --workspace`. The HTTP auth
suite exercises password login, durable sessions and signing, expired/revoked
credentials, REST/RPC impersonation, grants, hostile Surfaces, multiple viewers,
session collisions, private storage, and realtime membership.

`crates/api/tests/auth.fozzy.json` is a deterministic process-contract model.
`auth-host.fozzy.json` runs the real Rust regressions with Fozzy's host process
backend. Recorded host traces are under `backend/artifacts`; trace verification,
replay and CI check those recorded executions. The modeled fuzz run is not
cryptographic or Rust input fuzzing. Distributed exploration does not apply to
these single-node steps scenarios.

`crates/api/tests/surfaces.fozzy.json` models the Surface-auth process contract;
`surfaces-host.fozzy.json` runs the actual HTTP/RPC, delayed-admission, migration,
multi-batch cleanup, and native Surface regressions without starting servers.
Use the actual Fozzy determinism engine (not the FozzyLang compiler wrapper),
strict doctor/test first, then host execution with `CARGO_INCREMENTAL=0`, trace
verification, replay and CI. `backend/artifacts/surfaces-auth-bounded-host.fozzy`
records the focused implementation verification.
