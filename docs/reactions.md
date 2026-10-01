# Public Reactions

## Meaning

Reactions keep three independent dimensions instead of collapsing human feedback
into a score:

| Dimension | Values | Meaning |
| --- | --- | --- |
| Appreciation | `like`, `dislike`, `null` | The author's preference about an Object. |
| Engagement | `engaging`, `not_engaging`, `null` | The author's reported experience, not measured attention. |
| Position | `support`, `oppose`, `uncertain`, `null` | A human position about the Object's content, not evidence or a machine Judgment. |
| Certainty | integer `0..100`, or `null` | Optional self-reported confidence in that position. Requires a non-null position; zero is valid. |

All four keys are required, including explicit nulls. A partial update must not
silently clear another dimension. `uncertain` is an expressed position; `null`
means no current position. Clearing all four fields withdraws the current
reaction. A person may like an Object while opposing its content.

These are explicitly **public, signed reactions**. They are not the private
interest/history model. The host discloses publication before accepting a first
reaction. Withdrawal removes the current contribution to counts, not the signed
history or copies already observed by others. No reaction implies consent to
publish private reading history, interests, or Following relationships.

## State And Consistency

The current state is one value per `(author_id, object_id)`. It has a monotonically
increasing JavaScript-safe integer revision. Absent state is revision zero with
all-null values. Changed values advance the revision; an acknowledged no-op does
not. The caller supplies the expected revision and a 1-256 byte visible-ASCII
idempotency key. Actor identity comes from the authenticated host session, never
from a mutation body.

Actions and retry receipts are signed. SQLite commits the action, current
projection, summary changes, and receipt atomically. A repeated key with the same
request returns the original acknowledgment, including after restart or a later
change. Reusing a key for a different request conflicts. A stale expected revision
also conflicts. A fresh read, not an old retry receipt, describes current state.

This is a single-serving-node compare-and-set authority, consistent with the
current node's storage contract. It does not claim offline multi-writer CRDT
merge, federation, Sybil resistance, or proof that one identity equals one human.
The public summary counts each identity's latest nonempty value once. It exposes
separate choice counts and the number of certainty responses, never an average
certainty, universal veracity score, or confidence-weighted consensus.

## Transport

| REST | Result |
| --- | --- |
| `GET /objects/{id}/reactions` | Public `ReactionSummary`. |
| `GET /objects/{id}/reactions/actors/{actor}` | Public `ReactionRecord`, including the latest signed action or null for absent state. |
| `GET /objects/{id}/reactions/mine` | Authenticated host's `ReactionState`. |
| `PUT /objects/{id}/reactions/mine` | Authenticated mutation; body `{value, expected_revision, idempotency_key}`. |

Canonical RPC equivalents are `babble.social.reactions.summary.v1`, `.record.v1`,
`.mine.v1`, and `.set.v1`. Summary and mine take `{object_id}`; record takes
`{object_id, actor_id}`. Set takes `{object_id, value, expected_revision}` and
uses the envelope's idempotency key. REST and RPC share the same transaction
identity and receipts. The SDK exposes `social.reactions.summary/record/mine/set`.

Reads of public summaries and records are available to Surfaces through the
scoped broker. Mine and set are host-only: object/session-bound requests are
rejected, even for an Object owner. A Surface cannot silently react on a viewer's
behalf. Capability-mediated reaction consent and reaction subscriptions remain
separate protocol work; they are not simulated by the host controls.

## UI And Recovery

The active card owns the visible reaction controls. Offscreen cards do not
eagerly fetch personal state. Choices and counts update from validated backend
readback; failed writes do not invent success. A lost acknowledgment retains
the exact request for explicit retry. Conflict refreshes state and requires a
new choice. Account/session changes invalidate pending work; completion from a
previous card or login cannot overwrite the active view.

Signed history and retry receipts currently retain their full lifetime. Public
deployment still requires a retention policy, operational capacity testing,
backup/recovery drills, and the broader production-readiness audit. Public
reaction counts are not automatically fed into ranking or evidence Judgments.

Historical signatures are verified by the node against its identity transition
history. The public record endpoint exposes the signed action, but the public
identity endpoint does not yet return a complete key-transition proof bundle.
Standalone verification of rotated signing keys needs that separate proof path.
Coordinated rollback of the entire store also needs an external trusted checkpoint
to be detectable; local hash chains alone cannot establish freshness.

## Verification

- Fifteen focused store/node tests cover independent axes, withdrawal, no-ops,
  CAS, crash-safe installation, rollback, restarts, tampering, and signing keys.
- Thirteen API tests cover authentication, actor spoofing, strict request shapes,
  host-only mutation, concurrent requests, and retries shared across REST/RPC.
- The frontend's 222-test suite includes 21 focused reaction regressions; the
  SDK's 65 tests include wrappers and untrusted-Surface restrictions. Astro
  checking/build and generated-schema/fixture conformance pass.
- The actual API/Python/Astro/Aegis run verifies publication consent before any
  write, certainty zero, competing-tab recovery, withdrawal, restart, and
  containment within rounded cards at 390px and 320px. Its trace is
  `artifacts/live-stack-reactions-final-host.trace.fozzy`, verified, replayed,
  and CI-passed. Native/client traces are listed in the readiness ledger.

Browser geometry is not screenshot approval or physical-device gesture evidence.
