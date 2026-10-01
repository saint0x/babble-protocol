# Surface lifecycle and bridge contract

`BrowserBridgeHost.close()` is terminal and idempotent. It removes its listener,
clears every dispatch deadline before notifying cancellation observers, and
suppresses queued input and all late responses. Duplicate in-flight request IDs
receive `INVALID_INPUT` without another dispatch or releasing the original slot.
A timed-out dispatch retains its capacity slot until its promise settles. This
keeps dispatchers that ignore cancellation from exceeding the concurrency limit.

`BridgeDispatch` accepts an optional second argument `{ signal: AbortSignal }`.
Existing one-argument implementations remain valid. Scoped Surface dispatch
forwards the context unchanged. Close and timeout abort the signal; implementations
must observe or forward it to cancel underlying work. This cannot roll back an
operation already committed by a server.

Suspension removes the iframe and closes its bridge, including when reached
through `lifecycle.applyRuntimeEvent`, `applySession`, or resource pressure.
Eviction also disposes the mount and unsubscribes lifecycle observers. Repeated
unmount and recursive eviction hooks remove the frame once. This is execution
teardown, not a browser pause or an automatic checkpoint. `activate()` on a
disposed mount throws. Resume requires the host's checkpoint/session workflow and
a fresh mount with a runnable plan. The lifecycle controller can still reflect
backend suspended-to-active transitions without recreating the old iframe.
Cold and prefetched states both allow suspension, matching the backend.

Mount preserves prefetched, warm, and active plans and rejects suspended or evicted
plans before attachment. An attachment failure closes the bridge. Origin filters,
sandbox tokens, credentialless settings, and scoped authority remain enforced.

Browser Surface hosts admit one child-created MessageChannel port per mount.
The child transfers port2 in a window offer with exactly three fields:
`{type:"babble.surface.connect", protocol:"babble.rpc.v1", version:1}`.
Admission requires the current frame's exact `event.source`, the configured
origin policy, and exactly one transferred port. Isolated frames permit `null`
or the configured Surface origin; non-isolated frames require the exact origin.
The host sends `babble.surface.accept` on the offered port, the child replies
`babble.surface.confirm`, and the host sends `babble.surface.ready`. These controls
have the same protocol/version and exactly three fields. All RPC requests and
responses travel on that port. Window messages never carry host credentials or
capability grants, and there is no window RPC fallback. Duplicate or malformed
offers from this frame have all their ports closed. Admission is never replaced,
including when the first offer stalls before confirmation.

`MountedSurface.ready` resolves after confirmation (immediately for no-script
Static Surfaces) and rejects on timeout or teardown while connecting. The mount
option `handshakeTimeoutMs` and connector option `timeoutMs` default to 10000 ms
and accept integers from 1 to 120000. Timeout and terminal channel failures evict
the mount. A second frame load still evicts conservatively, but response isolation
does not rely on that event: an old-document reply uses the original port even
if a replacement document has appeared in the same WindowProxy before load.

`connectSurfaceBridge({parentOrigin, timeoutMs?, signal?})` returns a promise for
a BabbleTransport usable with `createBabbleSDK` or `createSurfaceSDK({transport,
...bindingOptions})`. The connector requires an exact HTTP(S) parent origin,
offers its port only to `window.parent`, and installs no window response listener.
Only its paired port can acknowledge the offer; sibling window messages cannot
complete the handshake. A raw public bootstrap that cannot know its embedder's
origin may target `"*"` for the secret-free offer; host admission checks remain
the same. The connector closes on abort (including after readiness), pagehide,
explicit close, messageerror, or a peer close. Close control is
`babble.surface.close` with the same protocol/version. Native port close events
are handled where available; abrupt disappearance without notification is not a
portable browser liveness signal. Handshake and RPC deadlines stay bounded.

Trust begins with the first admitted document. This does not authenticate remote
bytes or prove the configured integrity hash: initial redirects may determine
which document offers the first port, particularly for opaque sandbox origins.
Already admitted code can deliberately transfer its port to another document.
That delegation is intrinsic to transferable capabilities and cannot be prohibited
by this JavaScript protocol. The guarantee is prevention of accidental authority
inheritance through WindowProxy navigation, not confinement of a malicious
admitted document. Generic BrowserBridgeHost/Transport remain available only as
explicit caller-supplied endpoint mechanisms.

## Verification

Run from the repository root with `/Users/deepsaint/.cargo/bin/fozzy`:

```sh
fozzy doctor --deep --scenario sdk/tests/surface-hardening.fozzy.json --runs 5 --seed 42 --json
fozzy test --det --strict sdk/tests/surface-hardening.fozzy.json --json
fozzy run sdk/tests/surface-hardening-host.fozzy.json --det --seed 42 --proc-backend host --fs-backend host --http-backend host --record sdk/tests/surface-hardening-host.trace.fozzy --json
fozzy trace verify sdk/tests/surface-hardening-host.trace.fozzy --strict --json
fozzy replay sdk/tests/surface-hardening-host.trace.fozzy --json
fozzy ci sdk/tests/surface-hardening-host.trace.fozzy --json
npm --prefix sdk test
```

The deterministic scenario checks the scripted process contract; only the host
scenario actually builds and runs the SDK regressions. Tests use a fake DOM and
controlled timers, including all close/settle/deadline orderings for resolution
and rejection. They do not claim browser execution validation. Fozzy's property
fuzz mode in the installed build uses scripted subprocess handling even with host
flags, so it cannot substitute for this host-backed suite.

## Document Channel Verification

`document-channel.fozzy.json` is the strict scripted process contract and
`document-channel-host.fozzy.json` executes the complete SDK test suite against
the built SDK without rebuilding. Build the SDK first, then use these paths with
the doctor/test/run commands above and record `document-channel-host.trace.fozzy`.
The native Node MessageChannel tests transfer ports with structuredClone and
exercise actual asynchronous message delivery. Fake adapters and controlled
timers verify failures and cleanup deterministically. Host authority, rate limits,
deadlines and existing close regressions remain in the full SDK test suite.
These SDK checks do not claim browser navigation coverage; the parent owns the
separate live Aegis regression.

The document-channel process contract passed 32 Fozzy property-fuzz runs. These
runs cover scenario scripting, not mutations of the JavaScript protocol. Fozzy
`explore` rejects this steps schema because it requires a distributed scenario;
it is not counted as channel validation. Failure/close orderings are covered by
the controlled-timer JavaScript tests, and the actual implementation is exercised
by the host-backed trace.
