# Invocation Consent Decision

Status (September 30, 2026): **Four social methods and two browser consent/protocol paths verified in real API/Astro/Aegis acceptance against production frontend assets; physical native completion remains unverified.**

## Implemented Social Milestone

The SDK helpers `social.follow`, `social.unfollow`, `social.share`, and
`social.reply` now call `babble.social.{action}`. Their capability IDs and
declarations remain version 1. The four old `.v1` mutation methods return
`UnsupportedVersion` (`UNSUPPORTED_VERSION` on the wire), with the supported
method in error details. Social reads and other namespaces are not migrated by
this change. Mutation results contain a `PublicationReceipt`, not the former
capability receipt.

These four methods use durable one-use consent in the node, API, SDK and Astro
host. The broker ignores historical reusable social approvals as execution
authority and rejects new durable approvals for these capabilities. Permission
management shows **Ask every time**. Available, declared social capabilities can
be promptable at Surface admission without a durable grant; integrity, scope,
policy, denied/unavailable capabilities and other activation gates still apply.

`capabilities/src/invocation.rs`, `store/src/invocations.rs`, and
`node/src/invocations.rs` bind immutable normalized intent to the authenticated
actor/login, Object/version, document/Surface, method, scope, executor, policy and
server deadline. Local social execution atomically journals consumption,
aggregate quota accounting, signed effects and the publication receipt. Follow
and unfollow publish relationship assertions; reply and share publish text or
media Objects and their edges. Exact retries recover the original result rather
than producing another effect. The broader external `Running`/`Unknown` state
machinery also supports the two native browser methods described below; it does
not imply that the other external executors are implemented.

The registered [Surface document](surface-bridge.md) is now part of invocation
authority. `SurfaceInvocations` in the production `main.ts` dispatch rereads
authenticated intent, validates exact canonical bytes, and presents the dedicated
host DOM prompt outside the iframe. Allow Once, Deny and dismissal map to separate
decision/cancel operations. The controller bounds concurrent prompts, preserves
the original deadline, cancels on lost context, and suppresses late output. A
lost execution acknowledgement reconciled as completed can be returned only to a
still-live requester. No visible countdown is required for deadline enforcement.

Host composer submissions use explicit `host_action` documents and the submitted
intent as their decision, not reusable grants. Their request key hashes the
operation ID, method and payload: exact retries retain the key; edits change it.
The host client tries read-only completed-result recovery before registering an
execution document. This recovers an acknowledgement after document retirement
or expiry, without rebinding the old invocation or authorizing a new effect.

The real API/Astro/Aegis invocation acceptance run passed with a signed bundled
Object and backend effect readback. The trace is
`artifacts/invocation-consent-live-layout-host.fozzy` (strict verification,
replay and CI passed). Historical foundation and document-registration traces
are separate evidence; see the
[readiness ledger](production-readiness.md#social-invocation-milestone).

## Current Wire Contract

All endpoints require the authenticated originating login. Execution operations
also require exactly the matching source header: `x-babble-surface-document` or
`x-babble-host-document`. Host decision endpoints are not delegated SDK methods.

| Endpoint | Contract |
| --- | --- |
| `PUT /invocations/v1/documents/{uuid}` | Register a host-action document with `{object_id}`; 60-second lease capped by login expiry. Response includes `document_id`, `object_id`, `expires_at`, `renew_after_ms`. Expired/retired documents cannot revive. |
| `DELETE /invocations/v1/documents/{uuid}` | Retire the host document; returns 204. |
| `POST /invocations/v1/prepare` | `{origin, object_id, method, request_key, payload, timeout_ms}`; timeout defaults to 30,000 ms and must be positive and at most 30,000 ms. |
| `GET /invocations/v1/{id}/status` | Read the authenticated current invocation. |
| `POST /invocations/v1/{id}/decision` | `{decision: "allow_once" | "deny"}`. |
| `POST /invocations/v1/{id}/execute` or `/cancel` | Empty object body; execute stored intent or cancel before consumption. |
| `POST /invocations/v1/recover` | `{object_id, method, request_key, payload}`; no document header is allowed. Returns a completed host-action response for the original actor/login and exact intent, or 204 when none is recoverable. No document renewal, phase transition or execution. |

`origin` is `{kind: "surface", session_id, document_id}` or
`{kind: "host_action", document_id}`. Method-specific input payload includes
`author_id`, `target_object_id`, and text/media where applicable. The server
validates the author against authentication and stores normalized intent.

Responses contain `invocation_id`, `request_key`, `actor_id`, `object_id`,
`origin`, `method`, `payload`, `created_at`, `deadline`, `state`, `revision`, and
`result`. The result is null until completed, then `{object?, edge, receipt}`
with a `PublicationReceipt`. Initial social RPC consent is returned as
`PERMISSION_REQUIRED.details.invocation`; the trusted host handles the prompt
and returns the completed typed result to the original SDK request.

## Native Browser Methods

`babble.clipboard.write` and `babble.fullscreen.enter` share the canonical
invocation journal, authenticated document binding and one-use consent rules.
Capability declarations remain version 1; their old RPC v1 methods are rejected
as unsupported. Historical reusable grants cannot authorize these actions.
Available, declared browser capabilities are admitted lazily, with **Ask every
time** in permission management.

The browser endpoints use `/invocations/v1/browser`: `POST /prepare`,
`GET /{id}/status`, and `POST /{id}/decision`, `/dispatch`, `/ack`, `/cancel`.
Prepare and decision use the same input contracts as social invocations.
Clipboard payload is `{text}`. The complete payload must fit within 65,536 bytes
in both canonical encoding and UTF-8 JSON, including framing and JSON escapes;
the text itself is also bounded to 65,536 UTF-8 bytes before encoding. Fullscreen payload
normalizes to `{target_hint: null, navigation_ui: "auto"}` when omitted;
navigation accepts `auto`, `hide`, or `show`. A supplied hint is nonblank and at
most 256 UTF-8 bytes. It never selects arbitrary DOM: fullscreen targets the
originating host Object panel.

`BrowserInvocationResponse` is distinct from the social publication response.
It has the common intent/state fields, typed `result`, and `execution_ticket`.
Only the first successful dispatch response contains
`{dispatch_id, executor: "babble.browser.v1"}`. Status, retries, and recovered
Running/Unknown records never return another execution ticket. A lost dispatch
response therefore cannot justify another native attempt.

The rounded host prompt shows the exact literal clipboard text and actor.
**Allow once** obtains server approval and the dispatch ticket. A separate
**Copy once** or **Enter fullscreen** click invokes the native API synchronously
from that fresh gesture. Asynchronous authorization cannot reliably preserve
the transient activation required by [fullscreen](https://developer.mozilla.org/en-US/docs/Web/API/Element/requestFullscreen)
and [clipboard access](https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API).
Browser and social prompts share one permission slot per mounted Object.

Acknowledgement posts `{dispatch_id, result}`. Results are
`{kind: "clipboard_write", written: true}`, `{kind: "fullscreen_enter", entered: true}`,
or `{kind: "failed", code}` with `not_allowed`, `unavailable`, `context_lost`, or
`native_error`. Success is host-reported native completion, not server-observed
proof of an external effect. An uncertain acknowledgement can retry the same
acknowledgement, never the native action. This is at-most-once durable dispatch,
not exactly-once physical execution.

Closing, hiding, replacing or losing authorization cancels unstarted work and
suppresses late bridge results. In-flight native work retains the permission
slot until it settles. An already-started clipboard write cannot be undone;
late fullscreen completion exits only fullscreen owned by that Object host.
Backend, client and live-browser consent/protocol acceptance pass; see the
[readiness evidence](production-readiness.md#e2e-against-production-frontend-assets).
The browser suite deliberately leaves the machine's clipboard untouched, and
Aegis fullscreen activation returned `CAPABILITY_UNAVAILABLE` with an untrusted
click. These results do not establish physical native completion.

## Decision and Requirements

Implement lazy, per-invocation `ask_each_time` consent through a trusted host
decision and a durable invocation state machine. An approval authorizes one
specific operation, not a reusable capability grant. Blanket denial is a
temporary safety measure only and does not complete this milestone.

The governing requirements are broker mediation, declared scope, meaningful
host UI, and lazy sensitive permission requests in [spec.md](../spec.md),
"Capability System and Object SDK" and "Capability System and Host APIs",
plus realm/session binding, deadlines, cancellation, and mutation
idempotency in [sdk-spec.md](../sdk-spec.md), sections 4-8. Preserve the existing
rounded permissions UI and swipe frontend. Keep host credentials and decision
authority outside embedded Objects.

## Remaining Scope

Five `ask_each_time` methods are **not migrated to durable invocation consent**:

| Capability (version 1) | Current RPC method |
| --- | --- |
| `babble.payments.checkout` | `babble.payments.checkout.v1` |
| `babble.ai.generate` | `babble.ai.generate.v1` |
| `babble.ai.transcribe` | `babble.ai.transcribe.v1` |
| `babble.media.camera` | `babble.media.camera.request.v1` |
| `babble.media.microphone` | `babble.media.microphone.request.v1` |

Their raw RPC handlers return validated action descriptors and receipts, not
completed external effects. The social and browser invocation integrations do
not migrate these five methods. Notifications and embeddings are currently
`ask_once` and are outside this five-method list.

The signed-bundle browser run verifies four approvals, exact retries, immutable
preview, denial/cancellation/expiry/closure without writes, wrong-document,
other-actor and anonymous rejection, plus keyboard/pointer event isolation.
Prompt layout snapshots using production CSS pass at 320/390/1280 widths; these
are not physical-device gesture tests or full journeys at each viewport.
Same-actor/different-login, media and restart cases have backend coverage but
remain outside this focused embedded-app browser harness.
Broader work includes the five remaining external executors, lazy sensitive AskOnce
consent, external outcome reconciliation, retention/GC and deployment recovery.
The contract and matrix below specify the full target, not completed coverage.

## End to End Contract

1. **Separate admission from invocation.** Available `ask_each_time` methods are
   promptable and do not require durable grants to start a Surface. Keep actual
   activation prerequisites, including executable integrity and GPU activation,
   as admission gates. Apply lazy consent to sensitive `ask_once` methods too;
   expose per-method availability for fallbacks. Ignore historical/imported
   `ask_each_time` grants as invocation authority and reject new reusable
   approvals, while retaining history and AskOnce revocation behavior.
2. **Prepare authoritative intent.** Validate the method schema, manifest,
   concrete scope, executor availability, lifecycle, and quotas before prompting.
   Create a random challenge ID with a stable invocation key. Bind authenticated
   actor and originating login, immutable Object/version, Surface session and
   document instance, role/entry or bundle digest, capability/version/scope,
   method/version, canonical normalized payload hash, selected executor/provider,
   policy revision, and deadline. Freeze defaults and resource references. Store
   bounded payloads privately; execute those stored values. A request ID,
   child-supplied runtime ID, or claimed origin is not authority.
3. **Bind the admitted document.** Extend the existing source/origin-checked
   MessagePort handshake with a host-registered document instance. One session
   admits one document; replacement/navigation requires a fresh session. The
   trusted host injects this binding. Registration and decisions are host-only,
   never methods delegated through the embedded bridge.
4. **Decide through the host.** The initial call returns structured
   `PERMISSION_REQUIRED` with an invocation reference. Separate versioned
   prepare/decide/execute/status/cancel contracts use server-stored intent. The
   host presents actor, Object, exact action/recipient/content and applicable
   cost, Allow Once, and Deny outside the iframe. It enforces the immutable
   deadline without requiring visible countdown markup. Decisions require the originating
   authenticated login and exact challenge. Do not hold a node lock or HTTP
   worker while waiting for the user.
5. **Consume at execution.** Recheck credentials, document/session, policy,
   deadline, scope, and quota under the node execution lock. Non-consuming
   preflight must not consume again inside handlers. Pass checked invocation
   authority to node effects; native entry points must not retain a grant-only
   bypass. For local social effects, atomically journal consumption, quota debit,
   Object/edge/event, and result receipt. Aggregate quotas across invocations by
   actor/Object/capability, not by fresh challenge ID.
6. **Preserve host actions.** Replace the host social controller's reusable
   grants with an explicit `HostAction` context bound to its host document,
   login, controller Object, exact submitted intent, and invocation key. The
   host's own submit interaction may supply the decision. Missing Surface fields
   must never implicitly confer host authority.

## Retry and Lifecycle Semantics

The broader state model is `Pending -> Approved -> Running -> Completed | Failed | Unknown` with
`Denied`, `Cancelled`, `Expired`, and `Invalidated` terminal before consumption.
Implemented local social actions go directly from Approved to Completed:
consumption and completion commit together with the effect. Append
validated immutable phase records to the private store; reject illegal
transitions under the single-writer lock.

The same invocation key and intent returns the existing challenge, status, or
result, without another prompt, debit, or effect. Changed intent conflicts.
Transport request IDs may change on retry. Authenticate before result lookup;
do not require fresh consent to retrieve an already committed outcome. Revoked
execution contexts cannot execute again or receive late bridge output. A
separate actor-authorized history read may expose outcomes after session loss.
Retain tombstones for the retry horizon and reject expired keys rather than
silently treating them as new. Approval retries never revive terminal state.

Use an immutable server deadline established at ingress, bounded by the method
budget and consent TTL; approval waiting counts. Retries, heartbeats, and client
clock claims cannot extend it. Require an active visible Surface for prompting.
Invalidate pending/approved invocations on cancellation, navigation, port loss,
suspension/eviction, replacement, account switch, logout/session revocation,
lease expiry, permission/policy invalidation, or restart. Recheck at consumption
using the existing [execution authorization](../backend/crates/api/src/auth/surface.rs)
and [grant reconciliation](../backend/crates/node/src/grants.rs) boundaries.
Cancellation before consumption wins; after commit it cannot undo the effect.

## Executors and UI

Unsupported executors must report `CAPABILITY_UNAVAILABLE` before prompting,
never success with a descriptor. Implementing all required executors remains
part of the full production goal; hiding them is not platform completion.

External executors need durable dispatch records and provider idempotency where
available. Lost acknowledgement produces `Unknown` until reconciliation, not an
automatic second action. Browser APIs without transactional acknowledgement can
offer at-most-once dispatch, not guaranteed exactly-once external effects. Keep
execution authority in the host and recheck liveness before dispatch. Where an
async approval loses browser user activation, require a fresh trusted activation
button. Capture sessions must stop resources on lifecycle termination.

The four social methods have a dedicated invocation prompt controller, separate
from the management panel's stop-Surface flow. It rejects concurrent requests,
cancels on dismissal, and suppresses late decisions after context replacement.
Rounded styling, focus handling and event isolation pass the focused real-browser
acceptance described above. Social management shows Ask every
time, not durable Allow. Extending this behavior to the remaining executors is
still required; existing denied grant history is not a reusable approval.

## Implementation Boundaries

| Layer | Scope |
| --- | --- |
| Contracts | `capabilities`, `rpc`, API invocation schema, and generated SDK protocol types define the versioned social contract. |
| Node/store | `node/src/invocations.rs` and `store/src/invocations.rs` own intent, transitions, social publication, quotas and lifecycle/restart invalidation. Invocation keys are separate from durable grant/revoke retry records. |
| API/runtime | `api/src/invocations.rs` and its context/schema helpers authenticate source-specific operations and recovery. Runtime admits supported promptable social declarations without granting execution. |
| SDK/frontend | SDK  helpers, trusted document injection, `invocations.ts`, `surface-invocations.ts`, `invocation-prompt.ts`, `protocol.ts` and `main.ts` connect real host decisions and results. |
| Remaining acceptance | Physical-device gestures, broader embedded media/login/restart journeys, five remaining external executors, broader migration, recovery/retention policy and full requirement audit remain open. |

Prefer extending the existing publication journal. Storing consumption only in
the account SQLite database while effects commit through the file journal would
leave a two-store crash window. A shared transactional database/outbox is the
material alternative, but requires broader persistence ownership changes.

## Proposed Test Matrix

| Layer | Required cases |
| --- | --- |
| Broker/runtime | All eleven `ask_each_time` methods; lazy admission; legacy/imported grants cannot authorize; exact scopes; denied/unavailable behavior; AskOnce regression; aggregate quotas. |
| API authorization | Forged actor/Object/session/document/method/payload; same actor with another login; child decision attempts; native bypass; expired deadlines; prefetch/warm prompting denied. |
| Store/retries | Concurrent approve/deny/cancel/execute; identical retries; changed intent conflicts; one consumption/debit/effect; crashes before/after commit; restart recovery; tombstones; result retrieval after expiry. |
| SDK/lifecycle | Navigation and same-WindowProxy replacement; port closure; request cancellation; late decision/result; lease expiry; logout/account switch; revocation and restart; no resurrection. |
| UI/executors | Prompt flooding/deduplication; immutable summaries; dismissal/focus; rounded mobile/swipe behavior; real social writes; unavailable adapters; gesture requirements; capture teardown; provider acknowledgement loss without duplicate effects. |

For implementation acceptance, use Fozzy deep doctor with five seeded runs and
strict deterministic scenarios first, then recorded trace verification, replay,
CI, fuzz/explore/shrink, and host-backed effect/recovery checks. Use Aegis for
browser validation. Executed foundation and transport evidence is tracked in
[production readiness](production-readiness.md); the full matrix above remains
an acceptance requirement.
