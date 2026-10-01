Babble SDK — Technical Specification

Status: Draft implementation specification
Companion specification: Babble Protocol  — Technical Specification
Audience: Babble platform/runtime engineers and SDK engineers
Primary public SDK: TypeScript
Protocol/runtime implementation: Rust-first

1\. Purpose and Normative Context

This document specifies the developer SDK and host-facing API layer for Babble . It must be implemented in conjunction with the Babble Protocol  platform specification. The platform specification is normative for Object, Graph, Judgment, Lens, Runtime, identity, consensus, security, privacy, and capability semantics. This SDK must not redefine those semantics; it exposes them safely and ergonomically.

The SDK exists so third-party Objects can program against essentially every Babble platform capability that the host can safely expose. If the official client can perform an operation on behalf of an Object, that operation should have a documented capability/API unless exposing it would violate a security, privacy, integrity, or platform boundary.

The SDK is intentionally thin. It contains generated protocol types/codecs, validation, typed RPC transport, capability-aware ergonomic wrappers, lifecycle helpers, and developer tooling. Identity, graph execution, storage, realtime, Judgment providers, Jev, AI providers, payments, media access, GPU access, Lens execution, permissions, and other privileged behavior live in the Babble host/runtime.

2\. Design Goals and Non-Goals

Goals: fully type-safe TypeScript; small runtime footprint; complete programmability; capability-first security; browser-native operation; transport independence; generated protocol contracts; excellent inference/autocomplete; framework independence; versioned APIs; graceful capability discovery; composability; testability; support for generated/AI-authored Objects; Rust parity.

Non-goals: implementing Babble protocol logic independently in TypeScript; exposing host secrets; making TypeScript types a security boundary; coupling the SDK to React; coupling Judgment to Jev; requiring a particular transport; giving Objects unrestricted network/OS access; duplicating manually maintained Rust protocol models.

3\. Package Architecture

Initial packages:

@babble/protocol — generated protocol types, enums, schemas, codecs, IDs, canonical wire representations, validators, version constants. No host calls.

@babble/transport — minimal typed request/response/event transport abstraction. Implementations for browser bridge, dev WebSocket, native IPC where needed.

@babble/sdk — primary Object-author API. Thin namespaces over transport plus capability/lifecycle ergonomics.

@babble/runtime — host implementer interfaces, capability handlers, Surface hosting contracts, RPC router, policy hooks. Not required by normal Object authors.

@babble/dev — CLI, local host emulator, Object dev server, inspector, test harness, fixtures, profiler, permission simulator.

@babble/react — optional convenience bindings later. Never the core abstraction.

Rust crates remain authoritative for protocol behavior. TypeScript protocol artifacts are generated from one canonical schema pipeline derived from Rust/protocol definitions. Manual duplicate type definitions are prohibited.

4\. Source of Truth and Code Generation

Protocol structures such as Object, ObjectId, IdentityId, Edge, Judgment, SurfaceDescriptor, CapabilityId, Event, Lens metadata, ResourceRef, schemas, errors, and wire enums originate from the platform's canonical protocol definitions.

Generation pipeline conceptually: Rust/protocol schema → canonical intermediate schema → Rust validation fixtures \+ TypeScript definitions/codecs \+ JSON Schema where useful \+ protocol documentation.

Generated artifacts are checked for deterministic output in CI. Cross-language golden fixtures prove Rust and TypeScript serialize/deserialize the same bytes/data. Breaking schema changes require protocol versioning rather than silent regeneration.

Developer-friendly input types may be more ergonomic than wire types, but conversion into canonical protocol structures is explicit and tested.

5\. SDK Construction and Runtime Binding

Normal Object code imports a singleton or context-bound Babble SDK:

import { babble } from "@babble/sdk";

Internally the SDK is created from a BabbleTransport and a granted capability set. The host injects/binds the transport; Objects never construct privileged host transports with secrets.

For testing and advanced embedding, expose createBabbleSDK({ transport, capabilities, objectContext }).

SDK instances are scoped to Object identity/version, Surface/session, granted permissions, host/client, and lifecycle. They must not be transferable to another Object realm.

6\. Transport and Typed RPC

BabbleTransport is the only required bridge between SDK and host. Conceptual contract:

request\<M extends BabbleMethod\>(method: M, input: BabbleInput\<M\>, options?): Promise\<BabbleOutput\<M\>\>
subscribe\<E extends BabbleEvent\>(event: E, handler: (payload: BabbleEventPayload\<E\>) \=\> void): Unsubscribe
capabilities(): Promise\<HostCapabilitySnapshot\>
close(): void

Methods are versioned names such as babble.social.follow rather than unversioned magic strings. Convenience methods map exactly to documented RPC methods. Method versions and capability declaration versions are independent; the four social mutation helpers use method version 2 with capability version 1.

Browser Web Surfaces may use postMessage/MessageChannel or an equivalent isolated bridge. WASM Surfaces may map RPC to host imports/shared channels. Desktop may use IPC. Development may use WebSocket. Object code is transport-agnostic.

RPC envelopes contain request id, method/version, Object/runtime identity binding, payload, cancellation/deadline metadata, trace id where permitted, and structured error response. Host validates method, capability, schema, quota, and Object binding before dispatch.

The implemented browser bridge uses fresh per-attempt wire IDs, restores caller
IDs in responses, and sends cooperative cancellation on abort, timeout, and
close. See [Surface bridge](docs/surface-bridge.md#per-request-cancellation) for
the control message and its limits. Cancellation does not imply backend rollback.

For the implemented social  workflow, the initial RPC may return
`PERMISSION_REQUIRED.details.invocation`. The trusted host validates and rereads
that challenge, collects a decision outside the iframe, and executes stored
intent before resolving the SDK request. Authenticated `/invocations/v1` HTTP
prepare/status/decision/execute/cancel operations are host-only, not embedded
SDK authority. Source-specific document headers bind execution; the separate
host-action recovery endpoint is a read-only exception with no document header.
See [the implemented wire contract](docs/invocation-consent.md#current-wire-contract).

7\. Errors, Cancellation, Timeouts, and Idempotency

All SDK methods use a common BabbleError hierarchy with stable machine codes: CAPABILITY\_DENIED, CAPABILITY\_UNAVAILABLE, PERMISSION\_REQUIRED, INVALID\_INPUT, NOT\_FOUND, CONFLICT, RATE\_LIMITED, QUOTA\_EXCEEDED, TIMEOUT, CANCELLED, OFFLINE, PROVIDER\_UNAVAILABLE, UNSUPPORTED\_VERSION, INTEGRITY\_FAILURE, INTERNAL.

Methods accept AbortSignal where meaningful. Network/provider calls have explicit deadlines. Retriable errors declare retryability and optional retry-after.

Mutation APIs support idempotency keys where duplicate execution could matter, especially payments, publication, durable state mutations, and externally side-effecting calls.

Implemented social  invocations bind a stable request key to exact normalized
intent and the originating actor/login/context. Transport attempt IDs may change;
an exact retry does not create a second effect or extend the original deadline.
Changed intent under the same key conflicts. Consumption, social publication,
quota accounting and `PublicationReceipt` commit together. Cancellation before
consumption prevents execution; after commit it cannot undo the effect. A lost
acknowledgement can reconcile to the original result, but a closed or replaced
Surface must not receive late output. Host composer recovery can read a completed
outcome for the original login after document expiry without restoring execution
authority; it is not cross-login history access or automatic replay.

8\. Capability-Aware Type Safety

TypeScript should infer accessible namespaces from declared Object capabilities where practical.

defineObject({ capabilities: \["identity", "storage", "realtime"\] as const, ... }) produces a context whose babble type exposes those declared capabilities ergonomically. Calling undeclared location/payment APIs should be a compile-time error in strongly typed authoring contexts.

Runtime capability checks remain mandatory. TypeScript is developer assistance, never authorization.

Capabilities can be available, unavailable, promptable, granted with scope, or temporarily constrained. babble.runtime.capabilities() returns a live snapshot so Objects adapt across browser, mobile, desktop, independent clients, and future hosts.

Current lazy admission and durable one-use consent cover only the four social
mutations below. Historical/imported durable social grants do not authorize them,
and new reusable social approvals are rejected. Permission management displays
Ask every time. Other admission gates still apply. Extending this behavior to
all sensitive methods is a requirement, not a claim of current implementation.

9\. Object Definition and Authoring API

Provide defineObject() as the primary authoring primitive. It declares namespaced kind, schema/version, payload schema, state schema where applicable, capabilities, resources, relations, Surfaces, and lifecycle hooks.

Example shape conceptually:
defineObject({
  kind: "com.example.city",
  version: 1,
  capabilities: \["identity","storage","realtime","graphics.gpu"\],
  state: CityStateSchema,
  surfaces: { feed: feedSurface, fullscreen: worldSurface }
})

The builder produces validated canonical Object publication material but does not hide signing/publication. Build, preview, sign, and publish remain distinct developer actions.

10\. Surface Authoring and Lifecycle

SDK Surface helpers expose Preview, Feed, Expanded, Fullscreen, and permitted Background contexts defined by the platform spec.

Lifecycle hooks: onPrefetch, onWarm, onActivate, onSuspend, onResume, onEvict, onVisibilityChange, onResourceBudgetChange, onCapabilityChange.

SurfaceContext includes object identity/version, surface class, session id, typed payload/state access, granted SDK, resource budget, host feature snapshot, and cancellation/lifecycle signals.

Objects must assume suspension at any time. SDK helpers support checkpointing serializable local Surface state. GPU/resource handles are not assumed to survive suspension.

11\. Runtime Namespace

babble.runtime exposes host/version information, capabilities, Surface lifecycle state, current resource budget, feature detection, environment class, requestPermission where appropriate, and low-level RPC escape hatch.

The authenticated HTTP host renews Surface execution through `babble.runtime.surface.session.heartbeat.v1`, exposed as `runtime.heartbeatSurfaceSession` with a host Surface binding. This is not an embedded-Object permission: only the originating login's host can renew. See [host leases](docs/surface-leases.md) for timing, expiry and recovery semantics.

babble.rpc.call(method,input) is available to advanced developers for documented methods. Every high-level convenience method must map to a documented RPC primitive; no hidden SDK-only magic.

Raw RPC cannot bypass capabilities.

12\. Identity Namespace

babble.identity exposes current scoped identity, public profile resolution, allowed identity assertions, delegated session information, and signing requests where permitted.

Private keys never enter Object code. Signing occurs in the host/key subsystem after capability and user-policy checks.

APIs should distinguish public identity information from private/account information. An Object requesting basic identity must not automatically receive email, contacts, device identifiers, or unrelated profile fields.

13\. Object and Composition Namespace

babble.object resolves Object metadata/payloads, fetches compatible Surfaces/resources, validates Object schemas, publishes Objects, creates revisions/supersession, resolves composition, and exposes provenance.

Core operations conceptually: resolve(id), resolveMany(ids), publish(draft), validate(draft), resources(id), surfaces(id), provenance(id), compose(...), fork(id), remix(id), supersede(id,newDraft).

Object composition is first-class. Developers can resolve other Objects and reference them as components rather than copying payloads. Composition preserves Object IDs, provenance, capability boundaries, and independent lifecycle. An Object cannot inherit another Object's capabilities simply by containing it.

14\. Graph Namespace

babble.graph exposes typed graph reads and authorized writes. Basic operations include neighbors, incoming/outgoing edges, traverse, createEdge/assertRelation, remove/revoke mutable assertions where semantics permit, lineage, evidence, citations, forks, remixes, and social/semantic distance where available.

Provide both object-style queries and an ergonomic fluent builder. The builder compiles to explicit query structures; it is not a hidden query language.

Queries specify relation types, direction, depth, filters, limits, ordering, projection, and consistency/freshness requirements. Hosts enforce complexity budgets to prevent pathological traversals.

Semantic helpers may combine retrieval and Judgment, for example finding candidate contradictions, but results must preserve which portions are explicit graph facts versus Judgment-derived relationships.

15\. Social Namespace

babble.social exposes follow/unfollow, block/mute where appropriate to Object scope, share, reply, react, creator/profile lookup, relationship checks, and social events.

Current invocation implementation: `social.follow`, `social.unfollow`,
`social.share`, and `social.reply` map to `babble.social.follow`,
`babble.social.unfollow`, `babble.social.share`, and `babble.social.reply`.
Their `.v1` mutations return `UnsupportedVersion` (`UNSUPPORTED_VERSION`) and the
supported method in error details. Capability IDs remain `babble.social.follow`,
`babble.social.unfollow`, `babble.social.share`, and `babble.social.reply`, version 1.
One approval authorizes only the exact invocation, never a durable social grant.
Results contain `{object?, edge, receipt}` with `PublicationReceipt`; follow and
unfollow publish relationship assertions, while share/reply also publish an
Object. The remaining namespace description is the broader SDK target.

Clipboard and fullscreen helpers also use `` methods with durable one-use
dispatch and typed host-reported native outcomes; their v1 methods are rejected.
The host requests a fresh native-action gesture after server approval and never
reissues a dispatch ticket on retries. See the browser contract linked below.

Five `ask_each_time` RPC methods remain outside durable consent:
`babble.payments.checkout.v1`, `babble.ai.generate.v1`, `babble.ai.transcribe.v1`,
`babble.media.camera.request.v1`, and `babble.media.microphone.request.v1`.
Their raw RPC action descriptors are not completed external effects. The four social actions
have passed real signed-bundle API/Astro/Aegis acceptance, including exact retries
and no-write denial/cancellation/expiry/closure; see
[implementation status and evidence](docs/invocation-consent.md).

Objects must not receive a user's complete private social graph by default. APIs return only information permitted by capability scope and privacy policy.

Subscriptions/events include reactions, replies, shares, viewer/session joins where the Object is entitled to observe them, and relationship changes relevant to the Object.

16\. Storage and State Namespaces

babble.storage is namespaced durable key/value/blob storage for the calling Object. Typed helpers allow storage.get\<T\>(), set, delete, list, transaction where supported, quota, and blob references. Cross-Object storage access requires an explicit shared capability/protocol mechanism.

babble.state exposes Object/session state primitives distinct from arbitrary storage. It supports snapshots, subscriptions, optimistic mutation, CRDT-backed structures where available, and durable state adapters defined by the Object.

State APIs expose consistency semantics rather than pretending all state is strongly consistent. Types distinguish local, eventual/CRDT, authoritative, and consensus-backed state handles.

17\. Realtime Namespace

babble.realtime exposes rooms, presence, typed events, broadcasts, synchronized state channels, and optional peer messaging under host policy.

Developers define event maps with schemas. room.emit("block.placed", payload) and room.on("block.placed", handler) are fully inferred. Incoming events are runtime-validated, not trusted because TypeScript compiled.

Rooms support join/leave, participants, presence, broadcast, targeted messages where allowed, reconnect, resume tokens, backpressure, rate limits, and lifecycle-aware suspension.

Transport details—WebTransport/QUIC, WebSocket, peer transport—are hidden behind the host.

18\. Judgment Namespace and Jev Abstraction

babble.judgment is a first-class semantic compute API available to Objects with capability permission. Object developers never call Jev directly through Babble APIs.

Core evaluate accepts JudgmentState plus either registered Babble Judgment definitions or scoped custom structured questions permitted by host policy. Standard definitions return protocol Judgment records with definition/version, value, confidence, provider/model attribution as allowed, input hash, and timestamp.

Convenience constructors support boolean probability, choice distribution, and bounded score. Batch questions sharing state to exploit Jev/System-1 efficiency.

The host selects provider: cache, Jev, local model, specialist model, cascade, or ensemble. Objects may request capability classes/quality tiers but should not depend on a vendor unless explicitly using a nonportable extension.

Provider failure returns structured errors or policy-defined fallback. The SDK never silently converts provider failure into a semantic answer.

19\. Generative AI Namespace

babble.ai is separate from babble.judgment. Judgment evaluates structured meaning; AI generates or transforms content.

Potential APIs include generateText, generateStructured, embed, transcribe, synthesize, image/media generation where host capabilities exist, and streaming generation.

Hosts expose supported models/capability classes without requiring Objects to carry provider API keys. User/host policy controls provider, spending, privacy, and availability. Objects can bring external services only through separately declared network capabilities.

20\. Search and Discovery Namespaces

babble.search exposes lexical, semantic, Object-kind, identity, relation, and combined search within host/network permissions. Results carry Object IDs and enough metadata to resolve lazily.

babble.discovery exposes candidate/discovery primitives that are safe for Object applications: trending/emerging sets, related Objects, semantic neighborhoods, graph neighborhoods, and contextual recommendations where permitted.

These APIs do not grant an Object unrestricted access to another user's private recommendation state.

21\. Lens Namespace

babble.lens supports reading the active Lens metadata where permitted, defining/testing Lens policies, evaluating a Lens against supplied candidates, publishing Lens Objects, installing/selecting Lenses through user-mediated flows, and inspecting required signals/capabilities.

Third-party Objects cannot silently change the user's global Lens. Selection is an explicit host/user action.

Lens authoring APIs use constrained declarative policy where possible. Custom executable Lens logic runs sandboxed. Lens evaluation inputs distinguish public network signals from private local signals so private state cannot be exfiltrated through outputs.

22\. Media, Graphics, and Device Namespaces

babble.media exposes capability-gated camera, microphone, audio, video, screen/input surfaces where supported. babble.graphics exposes WebGPU/GPU feature discovery, Surface-bound graphics resources, and budget information rather than raw host privilege.

babble.location, babble.clipboard, babble.files, and other device APIs are separately permissioned. No broad "device" capability exists.

Handles are lifecycle-bound. Suspension may revoke camera/GPU/media handles. Objects must respond to capability and lifecycle events.

23\. Payments and Commerce Namespace

babble.payments provides host-mediated checkout, product/price resolution, purchase confirmation tokens/receipts where appropriate, subscriptions where supported, and entitlement checks.

Objects never receive raw payment credentials. Mutating payment calls require idempotency keys and explicit user confirmation according to host policy. Payment providers remain implementation details behind Babble contracts.

Commerce Objects can compose Product/Market/Checkout Objects without forcing all Babble Objects into a commerce model.

24\. Notifications Namespace

babble.notifications supports permission request, scoped notification registration, scheduling where host permits, cancellation, and Object-linked deep actions.

Notification permission is explicit. Objects cannot spam merely because they executed once. Host quotas, user controls, and abuse policy apply.

25\. Object-to-Object Communication and Composition

Babble needs safe composition beyond graph references. Define explicit interfaces/ports that an Object may export. Another Object can discover an exported interface by schema/version and invoke it through the host broker.

No Object receives direct memory/runtime access to another. Cross-Object calls are typed RPC with capability checks, quotas, lifecycle handling, and provenance.

This enables reusable Objects to behave like social software libraries: a music Object can expose playback/control, a Product Object commerce metadata, an Agent Object a structured invocation interface, or a simulation Object queryable state.

Interface IDs are namespaced and versioned. Composition must remain possible when implementations run in separate realms or on separate hosts.

26\. Events and Reactive Programming

All subscriptions return explicit unsubscribe/disposable handles and support AbortSignal. Event payloads are schema-validated.

Core event domains include runtime lifecycle, capability changes, Object state, graph changes, realtime rooms, social interaction, notifications, and host connectivity.

The SDK should provide async iterables in addition to callbacks where useful, enabling for-await consumption without framework dependence.

27\. Framework Integration

The SDK core is framework-agnostic. Optional @babble/react may expose hooks/providers for identity, Object resolution, state, realtime rooms, capability state, Surface lifecycle, and async resources.

Equivalent adapters can exist for other frameworks without changing protocol APIs. Never encode React component models into Object/Surface protocol definitions.

28\. Browser Host Integration

For Web Surfaces, @babble/sdk runs inside the sandboxed Object realm. The Babble host owns the parent/runtime broker. Establish a handshake binding ObjectId/version, Surface/session, SDK protocol version, allowed origin/realm, capability grant, and transport channel.

Messages from arbitrary windows are rejected. Validate origin/channel, request schema, runtime binding, sequence/replay protections where needed, and capability authorization.

The SDK should support tree-shaking so an Object using storage/realtime does not ship unnecessary helpers. The protocol package may be split internally/generated with export maps while preserving a coherent public package.

29\. WASM and Rust Object Support

TypeScript is the flagship SDK, not the only language. Rust Objects use a native babble-sdk crate with equivalent capability/RPC contracts. WASM host imports should correspond to the same versioned methods exposed to TypeScript.

Cross-language conformance tests ensure TS and Rust Objects observe equivalent behavior and errors. The TypeScript SDK must not introduce features impossible to express through the underlying capability protocol.

30\. Local Development and Emulator

@babble/dev provides babble dev, babble build, babble validate, babble test, babble publish, and inspection commands.

The local host emulator supplies deterministic identities, graph fixtures, Object store, storage, realtime rooms, local and remote Judgment providers, AI adapters, capability prompts, permission policies, resource budgets, lifecycle transitions, and network failure simulation.

Developers can switch Judgment modes across real provider implementations: deterministic local provider, Jev development provider, and configured production providers. Tests should not require paid Jev calls unless explicitly integration-tagged.

Inspector views: RPC trace, capability grants, Object manifest, graph relations, Judgment traces, Surface lifecycle, memory/CPU/GPU budgets, realtime events, storage, and Lens decisions.

31\. Schemas and Runtime Validation

Every externally supplied payload is runtime-validated: RPC inputs/outputs, custom Object payloads, state/events, cross-Object interfaces, realtime events, manifests, and generated content crossing trust boundaries.

The SDK should use generated validators from the canonical schema pipeline. Developer custom schemas may use a supported schema library/standard but must compile to a runtime validator and serializable schema description where interoperability requires it.

Never rely on erased TypeScript generics for safety. A get\<T\>() generic improves DX but does not prove stored bytes are T unless a schema/codec is supplied or bound by Object definition.

32\. Versioning and Compatibility

Version protocol schemas, RPC methods, capabilities, exported Object interfaces, Judgment definitions, and SDK packages independently but coherently.

SDK semver communicates package compatibility; protocol version negotiation communicates runtime interoperability. A new SDK can talk to an older host only through methods/capabilities both advertise.

Versioned method names allow additive evolution. Deprecation metadata is machine-readable and surfaced in development. Removing a method requires an explicit major/protocol compatibility process.

Objects declare minimum/optional host capabilities and can provide fallback Surfaces. Unknown future capabilities are ignored/transported rather than causing parser failure.

33\. Capability Quotas and Resource Budgets

Capability access includes quantitative policy. APIs expose relevant limits: storage bytes, realtime message rate/size, Judgment requests/tokens/cost class, AI generation budget, network origins/bandwidth, GPU/memory budget, background time, notifications, and payment operation constraints.

Quota errors are structured. SDK helpers may expose remaining/approximate budget where the host can safely do so. Objects must not infer unlimited resources from a granted capability.

Surface resource budget changes are events because mobile thermal pressure, backgrounding, memory pressure, or feed scheduling can reduce available resources dynamically.

34\. Security Invariants for the SDK

The SDK never contains Babble root secrets, user private keys, Jev API secrets supplied by the host, payment credentials, or unrestricted host tokens.

Every privileged request is authenticated by the bound runtime channel and authorized by the host capability broker. Object-supplied ObjectId/userId fields never substitute for host-bound caller identity.

RPC schemas reject prototype-pollution-style structures and malformed payloads. Binary/resource sizes are bounded before allocation. Subscriptions are quota-controlled. Cross-Object calls cannot create confused-deputy capability escalation.

Web transport uses isolated channels/realms and strict origin handling. CSP/network policy remains host-controlled. SDK network conveniences cannot bypass declared babble.network permissions.

35\. Privacy Invariants

SDK APIs make data sensitivity visible. Methods that can expose private user state require explicit capabilities/scopes. No analytics helper automatically captures payloads, user-model features, private graph edges, JudgmentState, camera/microphone data, or generated prompts.

Judgment/AI calls disclose to developer tooling whether execution is local or remote when policy permits and what data class may leave the device. Objects cannot force a remote provider when user policy requires local-only operation unless the Object explicitly declares that remote service as a requirement and the user accepts it.

36\. Observability and Developer Tracing

Development builds attach trace ids across SDK → transport → capability broker → service/provider. babble/dev inspector can show timing, payload schema (with sensitive fields redacted), cache/provider selection for Judgment, retries, quotas, and errors.

Production tracing is sampled/minimized and obeys privacy classes. Object developers receive metrics about their Object, not unrestricted host/user telemetry.

37\. Testing Strategy

Package unit tests cover builders, validators, errors, lifecycle, transport, and type-level behavior. Type tests prove capability inference and invalid calls fail compilation.

Golden conformance fixtures prove Rust/TS protocol equivalence. Transport contract tests run the same suite against in-memory, browser bridge, dev WebSocket, and native IPC adapters where applicable.

Security tests attempt forged capability grants, spoofed Object identity, malicious postMessage sources, malformed RPC, replay/duplicate mutation, quota bypass, cross-Object access, oversized payloads, subscription leaks, and cancellation races.

SDK integration tests use the emulator. Provider-specific Jev tests are isolated and optional in normal CI. Deterministic Judgment fixtures make core tests reproducible.

Browser matrix tests cover current major engines supported by Babble. WASM/JS interop, worker execution, suspension/resume, and WebGPU capability fallbacks are tested explicitly.

38\. Performance and Bundle Budgets

The SDK must remain a thin wrapper. Avoid embedding large model runtimes, graph engines, UI frameworks, or duplicate protocol implementations in @babble/sdk.

Track minified/compressed bundle sizes per package and namespace. Use ESM, export maps, side-effect-free modules, and tree-shaking. Heavy optional codecs/helpers are lazy or separate exports.

RPC overhead should be negligible relative to host operations. Batch APIs exist for Object resolution, graph queries, Judgment questions, storage operations, and other naturally batchable workloads. Avoid N+1 host crossings.

Event handling must apply backpressure and avoid unbounded queues. Surface suspension stops unnecessary subscriptions unless explicitly retained by allowed background capability.

39\. Documentation Contract

Every public method documents capability requirement, privacy implications, input/output schema, errors, idempotency behavior, lifecycle constraints, host support/fallback, version, and at least one example.

Documentation is generated partly from canonical RPC/capability metadata so code and docs cannot drift silently. Examples are compiled/tested in CI.

Provide guides for: first Object; interactive Feed Surface; fullscreen app; graph/evidence Object; realtime multiplayer; Judgment/Jev usage; AI generation; Object composition; payments; Lens authoring; capability permissions; browser compatibility; Rust/WASM Object; testing/publishing.

40\. Publishing and Supply-Chain Workflow

babble build validates manifests/schemas, compiles Surface bundles, computes resource hashes, runs static capability/network analysis, produces canonical Object draft, and emits a human-readable permission/resource report.

babble sign requests signing through configured developer identity tooling. babble publish uploads resources/content-addressed artifacts and publishes the signed Object/event through configured Babble node/gateway.

Published resources are immutable by hash. Updating executable code creates a new Object version/supersession relationship. Package lockfiles/build metadata may be included in provenance to improve reproducibility.

41\. Example End-to-End Object Flow

Developer defines City Object with identity, storage, realtime, and graphics.gpu. Build generates canonical manifest and hashed Feed/Fullscreen resources. Publish signs the Object and announces it to Babble.

A user encounters it. Host validates Object/signature/resources, selects Feed Surface, prefetches bundle, creates sandbox, performs SDK handshake, grants identity/storage/realtime/GPU according to policy, and activates the Surface.

Object calls babble.identity.current(); host returns scoped identity. It joins babble.realtime room; host selects transport. It loads typed state from babble.storage/state. It renders through WebGPU. When scrolled away, host emits suspend; Object checkpoints state and releases transient resources. When reopened fullscreen, a new/richer Surface context activates against the same Object and shared state.

If the Object asks babble.judgment to classify a player-created structure, host may return cache, local model, Jev, or ensemble output. The Object receives the same Babble Judgment shape and never needs a Jev API key.

42\. Engineering Boundaries with the Platform

Platform team owns canonical protocol types, capability semantics, runtime broker, identity/key security, graph/state services, Judgment provider orchestration, Jev adapter, Lens engine, network/consensus, host security, and platform conformance fixtures.

SDK team owns generated TS artifacts, ergonomic wrappers, transport clients, Object/Surface authoring API, capability-aware typing, lifecycle helpers, dev CLI/emulator/inspector integration, docs, and cross-language/browser conformance.

Changes crossing the boundary begin with canonical protocol/RPC/capability definitions, not ad hoc SDK methods. The SDK must never invent behavior the host does not specify.

43\. Recommended Initial Repository Layout

sdk/
  packages/
    protocol/
    transport/
    sdk/
    runtime/
    dev/
    react/        optional/later
  examples/
    hello-object/
    realtime-game/
    claim-evidence/
    webgpu-world/
    judgment-app/
    composed-drop/
  tests/
    conformance/
    browser/
    security/
    type-tests/
  tooling/
    codegen/
    schema/
    docs/

The monorepo may live beside the Rust workspace or inside one Babble monorepo. Prefer one CI graph so protocol changes regenerate/test SDK artifacts atomically.

44\. Implementation Sequence

Phase 1: canonical RPC/capability schema; @babble/protocol generation; in-memory transport; errors/version negotiation.
Phase 2: @babble/sdk core, runtime/capabilities, identity, Object, graph, storage; type tests.
Phase 3: browser bridge \+ Surface lifecycle \+ dev host emulator.
Phase 4: realtime/state and typed events.
Phase 5: Judgment API \+ Jev-backed host integration \+ deterministic test provider.
Phase 6: search/discovery/Lens/social APIs.
Phase 7: media/GPU/device/notifications/payments as platform services become ready.
Phase 8: cross-Object interfaces/composition, Rust/WASM parity, advanced developer tooling.
Phase 9: optional framework bindings and ecosystem polish.

SDK and platform development should proceed vertically: implement one capability end-to-end from Rust host → canonical RPC definition → TS generation → SDK wrapper → emulator → integration test, rather than writing SDK surfaces ahead of platform services.

45\. Definition of Done / Acceptance Criteria

The initial production SDK is complete when:
1\. @babble/protocol is generated from canonical platform definitions and passes Rust/TS golden fixtures.
2\. @babble/sdk operates over at least browser and dev transports without application-code changes.
3\. Object capability declarations produce useful compile-time SDK narrowing while runtime authorization remains host-enforced.
4\. A developer can define, build, validate, sign, publish, resolve, fork/remix, and compose Objects.
5\. Feed and Fullscreen Surfaces receive lifecycle-safe typed contexts.
6\. Identity, social, graph, storage/state, realtime, Judgment, search/discovery, Lens, and runtime namespaces work end-to-end for their initial platform implementations.
7\. Jev-backed Judgment is usable through babble.judgment without Jev types/keys appearing in Object code.
8\. Provider replacement/cache/local fallback returns the same Judgment contracts.
9\. A browser Object can execute live in-feed with sandboxed capabilities.
10\. Typed realtime custom events work and are runtime validated.
11\. Cross-Object composition preserves independent identity/provenance/capabilities.
12\. Raw versioned RPC is documented and cannot bypass capability policy.
13\. Emulator runs examples without requiring production infrastructure.
14\. Security suite demonstrates caller binding, capability enforcement, isolation, quota enforcement, and hostile-message rejection.
15\. SDK bundle/runtime overhead meets agreed budgets and tree-shakes unused namespaces.
16\. Examples/docs are compiled/tested in CI.
17\. A Rust/WASM Object can access equivalent core capabilities through the same underlying method contracts.
18\. An engineer can add a new platform capability by defining the canonical method/schema, implementing the host handler, regenerating protocol artifacts, and adding a thin SDK wrapper without redesigning the SDK.

46\. Final SDK Principle

The Babble SDK should feel enormous in capability and tiny in implementation.

To Object developers it exposes a programmable social operating environment: identity, graph, social interaction, Objects, composition, storage, realtime, semantic Judgment, AI, discovery, Lenses, media, GPU, device features, payments, notifications, and future capabilities.

Underneath, it remains generated types \+ validation \+ typed RPC \+ capability-aware ergonomics.

The host does the work. The protocol defines the meaning. The SDK makes it programmable.
