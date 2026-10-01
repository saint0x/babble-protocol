# Document-Bound Surface Bridge

Browser Surfaces use one child-created `MessageChannel` per mount. The host
accepts an offer only from that iframe's current `contentWindow` and its allowed
origin. A window message is only a connection offer, never an RPC request or
response. All subsequent traffic uses the admitted port. No account token is
sent into the frame.

The port distinguishes a document's connection from the navigation-stable
`WindowProxy`. A replacement document cannot establish another connection in
the same mount, and late responses are sent only to the original port. Another
iframe load still evicts the mount. Suspension, eviction, errors, timeout, and
close dispose the bridge and abort cooperative dispatch; committed server work
cannot be undone by browser cancellation.

## Handshake

Control messages have exactly three fields:

```json
{ "type": "babel.surface.connect", "protocol": "babel.rpc.v1", "version": 1 }
```

1. The child creates a channel and transfers exactly one port with `connect`.
2. The host admits at most one offer and sends `babel.surface.accept` on the port.
3. The child returns `babel.surface.confirm` on its retained port.
4. The host registers its randomly generated document ID with the authenticated
   node. No RPC can dispatch while registration is pending.
5. After the node acknowledges the exact session/document binding, the host
   enables the scoped RPC dispatcher and sends `babel.surface.ready`.

All control messages use the same protocol and version fields. Neither requests
before confirmation nor legacy window RPC calls can dispatch. The canonical RPC
schema is unchanged; method, capability, identity, Object, and session scoping
still apply. The transport assigns a fresh wire request ID for each attempt and
restores the caller's ID on the response.
Duplicate offers do not replace the admitted port. `babel.surface.close` closes
the connection. A close notification is best effort, not a durable mutation.

`BrowserSurfaceHost.mount()` returns `MountedSurface.ready`. The default handshake
deadline is ten seconds; configured deadlines must be positive integer milliseconds
and at most two minutes. Static, script-disabled Surfaces need no RPC handshake.
The Astro controller waits for readiness before warm/active transitions and keeps
lease renewal running while connecting. Close, account changes, or lease failure
tear down pending startup without allowing late completion to reactivate it.

## Server Document Binding

The host implements `registerDocument(documentId, signal)` when mounting an
executable Surface. Astro sends authenticated `PUT
/runtime/surfaces/sessions/{session_id}/document` with
`{ "document_id": "<lowercase UUID>" }`. The response must contain that exact
`session_id` and `document_id`. The callback runs only after the admitted port
confirms, and its network request shares the handshake cancellation signal.
Static, script-disabled mounts do not register a document.

The node binds one document to one Surface session and its originating login.
An identical registration can be retried; a different document conflicts.
Suspension, eviction, lease expiry, or login loss cannot authorize replacement.
Reopening requires a fresh session. Existing unregistered sessions cannot make
Object-bound HTTP RPC calls until their host registers them.

The SDK's scoped host dispatcher supplies `surfaceDocumentId` in its trusted
dispatch context. `HttpRpcTransport` puts it in `x-babel-surface-document` for
requests bound to both an Object and a Surface session. The ID is never copied
from a child request, included in child handshake messages, or used as a login
credential. Host wrappers must preserve it when replacing the cancellation
signal. The API rejects missing, duplicate, malformed, unregistered, or mismatched
bindings and checks the binding again under the node execution lock. Host session
management remains header-free and requires the original authenticated owner.

This completes document registration and HTTP execution binding, not per-action
consent. It does not make browser cancellation transactional or consume an
`ask_each_time` approval; those remain in [invocation consent](invocation-consent.md).

## Client Integration

Use the exported `connectSurfaceBridge({ parentOrigin, signal, timeoutMs })` to
obtain a `BabelTransport`, then supply that transport to `createSurfaceSDK` or
`createBabelSDK`. `parentOrigin` must be the exact expected HTTP(S) origin, not
`*`. Keep the normal prepared Surface plan and binding parameters; the host
independently binds requests to its admitted Object, session, identity and grants.
Abort or close the transport when it is no longer needed. The connector also
closes on `pagehide` and fails boundedly if its parent never responds.

The bundled, public RPC example uses the same handshake directly in its
content-addressed script. Its initial offer targets its parent with `*` because
it has no private data or out-of-band host-origin configuration; no RPC or result
uses wildcard window delivery. Applications that require an authenticated host
origin must use the SDK's explicit origin pin.

Legacy window-message Surface clients must be republished with this handshake.
Existing signed Objects and resources are not rewritten, and there is no unsafe
automatic fallback. Generic caller-supplied `BrowserBridgeHost` and
`BrowserBridgeTransport` endpoints remain available outside browser Surface mounts.

## Per-Request Cancellation

An aborted signal, local timeout, or transport close sends this control message
for each request already sent:

```json
{ "type": "babel.rpc.cancel", "protocol": "babel.rpc.v1", "id": "<wire request ID>" }
```

The host accepts only this exact shape and the originating request's origin,
then aborts its dispatcher signal and suppresses late responses. Unknown,
duplicate, and premature cancellations have no effect. Noncooperative dispatch
continues occupying its capacity slot until it settles; cancellation is not a
way to bypass the in-flight limit. Dispatchers must observe or forward the signal.

Fresh wire IDs also prevent stale responses from matching a later request when
the caller reuses its own ID. Older hosts may ignore cancellation, but cannot
alias those attempts. Idempotency keys and payloads are preserved. A synchronous
send failure rejects only that attempt and does not send cancellation.

This is cooperative transport cancellation, not durable invocation cancellation
or transaction rollback. Already-admitted backend work may still commit after
the HTTP connection closes. Per-use consent remains separate work described in
[invocation consent](invocation-consent.md).

## Trust Boundary

This binds access to the first admitted document's port. It does not prove that
an arbitrary remote URL returned the approved bytes, prevent an initial redirect,
or stop already-admitted code from deliberately transferring its port elsewhere.
Remote resource integrity/redirect policy remains a separate requirement. The
host must continue enforcing capabilities even over an established channel.

This design follows the web platform's explicit transferable-port model.
The [HTML Standard's capability discussion](https://html.spec.whatwg.org/multipage/web-messaging.html#ports-as-the-basis-of-an-object-capability-model-on-the-web)
describes limited authority and deliberate delegation through ports;
[MDN's MessageChannel reference](https://developer.mozilla.org/en-US/docs/Web/API/MessageChannel)
documents channel construction and transfer. Both were read with Aegis on
September 30, 2026. These sources explain the mechanism, not a security audit of
Babel's implementation.

## Verification

### Server Document Binding (September 30, 2026)

- 221 API tests, 145 SDK tests, and 638 frontend tests pass. Astro check reports
  zero errors/warnings and two existing hints; the production build passes.
- The real API/Astro/Aegis suite verifies registration before readiness, trusted
  document headers, successful backend reads, immutable registration retries,
  originating-login isolation, and eviction. The separate verified-bundle suite
  exercises the snapshot gateway, authenticated RPC, CSP, tampering, wrong-origin
  and wrong-source negative controls, eviction, and account revocation.
- Recorded host traces pass strict verification, replay, and CI:
  `../backend/crates/api/.fozzy/documents-host.fozzy`,
  `../artifacts/surface-document-client-verified-host.fozzy`,
  `../artifacts/surface-document-browser-host.fozzy`,
  `../artifacts/live-stack-surface-document-host.fozzy`, and
  `../artifacts/verified-bundle-document-current-host.fozzy`.
- These checks do not establish durable per-invocation consent. See
  `invocation-consent.md` for the remaining implementation boundary.

### Earlier Channel Milestone

- Full Rust workspace, 289 frontend tests, 92 SDK tests, generated contracts,
  independent fixture conformance, Astro checking and production build pass.
  Astro retains two async-conversion hints. Changed API files pass formatting;
  the package-wide formatter still flags the pre-existing `lib.rs` re-export order.
- Native MessageChannel and controlled-timer tests cover admission, handshake
  ordering, authority binding, malformed/duplicate offers, aborts, deadlines,
  pending dispatch and terminal cleanup. Controller tests cover readiness versus
  close, account changes, lease failure, replacement and late settlement.
- Real Aegis opaque-origin navigation executes the replacement document while
  its load is deliberately held. Native-send checkpoints and replacement-side
  acknowledgements precede assertions. The replacement cannot dispatch through
  window messages or a second port, and cannot receive the old pending response.
  Load completion evicts the old mount. Deliberately leaky-window and duplicate-
  admission controls trigger the corresponding security assertions. Instrumentation
  checks the selected send paths, not arbitrary future tasks or remote integrity.
- The complete API/Python/Astro/Aegis run verifies the bundled signed Surface's
  actual handshake, successful backend RPC response and active lifecycle,
  including reopening after login restoration. Existing leases, social flows,
  source agreement, publishing and rounded-card layouts continue to pass.
- Strictly verified, replayed, CI-passed host traces:
  `../sdk/tests/document-channel-host.trace.fozzy`,
  `../artifacts/document-bridge-controller-host.fozzy`, and
  `../artifacts/document-bridge-barriers-verified-host.fozzy`, plus
  `../artifacts/live-stack-document-bridge-verified-host.fozzy`.
  Scripted doctor/test/fuzz checks validate orchestration, not browser scheduling.
  Distributed exploration is unsupported for these step scenarios.

Earlier failure traces are retained. The first full runs waited for a transient
RPC status label that activation replaced with `Ready`; assertions now inspect
real port traffic and backend success instead. A standalone recording also failed
in Aegis navigation before reaching any security assertion; its rerun passes.
The full verified run also includes the corrected navigation fixture and both
negative controls.
