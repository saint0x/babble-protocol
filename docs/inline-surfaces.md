# Inline Surfaces

The Astro host puts an admitted executable Surface inside its Object's primary
card. Rounded tonal cards, horizontal Object navigation, and smaller vertical
conversation rows remain the layout. Expansion changes the active card's size,
not the mounted iframe, so running application state is retained.

The host owns close, expand/collapse, retry, and runtime inspection. Surface
controls do not initiate deck swipes or arrow-key navigation; the surrounding
deck remains navigable. Close restores the reading anchor and initiating control.
Opening a public Object outside the loaded feed temporarily presents that Object;
closing returns to the previous feed and reading position without inventing rank
or adding the Object to private Following.

## Ownership and Teardown

`frontend/src/app/surfaces.ts` owns each opening separately. Preparation, session
creation and warm/active transitions cannot mount a stale opening after account
change, navigation, replacement or close. A late session creation is retained for
cleanup. Local teardown is synchronous; remote eviction waits for pending start
or transition work, so an old activation cannot race after eviction. Errors in
cleanup are reported, not treated as successful termination.

Bridge dispatch forwards the SDK cancellation signal through authenticated HTTP.
Each opening is bound to its initiating account token; embedded code never receives
that token. Closing cancels cooperative work and suppresses late responses. It
cannot roll back a mutation already committed by the server.

SDK suspension now removes the executable iframe and closes the bridge, including
direct runtime lifecycle events. It is not a pretend JavaScript pause. Reopening
uses a new session and mount. The [document-bound bridge](surface-bridge.md)
admits one child-created port, and the controller waits for its confirmation
before activating the session. Replacement documents cannot use window RPC or
replace that channel while their load is pending. A subsequent iframe load also
evicts the mount. See the [SDK contract](../sdk/tests/surface-hardening.md) for
initial-document trust limits and checkpoint requirements.

Host reply/share/follow actions use an Object binding with a null Surface session;
they no longer invent session IDs. Explicit Surface bindings must pass the API's
session checks even when the caller authored the Object. Backend authority and
originating-device cleanup are described in [the auth contract](../backend/AUTH.md).

## Evidence

- 159 frontend tests cover lifecycle interleavings, production dispatch/SDK
  integration, input ownership, ordinary social bindings, and return placement.
  Astro check and production build pass with two async-style hints.
- 62 SDK tests cover isolation, direct runtime teardown, dispatch cancellation,
  late settlement, duplicate IDs, timeouts and bounded in-flight accounting.
- The real API/Astro/Aegis suite checks inline placement, unchanged iframe identity
  during expansion, accessible exit, authoritative backend eviction and a fresh
  session on reopen. Existing publication, profiles, Following, media and reply
  workflows continue to pass. Rounded-card geometry is checked at 1280, 390 and
  320 pixels; the narrow checks use same-origin frames, not physical devices.
- `artifacts/live-stack-inline-surfaces-final-host.trace.fozzy` passes strict
  verification, replay and CI. The earlier trace is retained: its actual live
  assertions passed, but the scripted `proc_when` incorrectly expected empty
  stdout. `tests/live-stack-host.fozzy.json` deliberately has no scripted response;
  it executes the real process and asserts its exit status. Scripted doctor/test
  and fuzz runs validate orchestration only. Distributed exploration does not
  apply to this steps scenario.
- `artifacts/surface-authority-final-host.trace.fozzy` passes verification,
  replay and CI for the real Rust HTTP/RPC authority suite. It covers originating
  login isolation, expired/revoked tokens, queued calls invalidated after
  admission, legacy ownership migration, bounded cleanup past live rows, and
  retirement only after eviction, including retry of a missed retirement write.

## Still Open

This proves an embedded Web Surface host, not every executable Object experience.
The seed Surface exercises bridge RPC, not a complete game. Browser fullscreen,
pointer lock, all protocol host actions, explicit checkpoint restoration and
resource-budget enforcement still need end-to-end evidence. The installed Aegis
surface offers DOM/geometry checks but no screenshots or native viewport sizing;
visual and physical-device review remain open.

[Host leases](surface-leases.md) now bound abandoned HTTP sessions independently
of account expiry. Reconnect uses explicit fresh-session retry. Saved-state
recovery and remote-code integrity/initial-redirect policy remain open; the port
handshake is not proof of the first admitted document's bytes.
