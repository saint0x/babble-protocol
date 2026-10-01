# Surface Host Leases

Authenticated HTTP Surface sessions require an explicit host lease. Creation
reserves 60 seconds, capped by the originating login's expiry. The host renews
through `babble.runtime.surface.session.heartbeat.v1` with an empty payload and a
host binding containing `surface_session_id`, or through
`POST /runtime/surfaces/sessions/{id}/heartbeat`.

The response is `{ lease: { session_id, expires_at, ttl_ms, renew_after_ms } }`.
`expires_at` is an RFC3339 UTC deadline; `ttl_ms` is at most 60,000 and
`renew_after_ms` is at most 15,000, shortened near account expiry. Heartbeats are
non-idempotent renewals, not durable content mutations. Reads and ordinary Object
RPC do not renew the lease. Embedded code cannot heartbeat its own session.

Only the originating authenticated login can renew a prefetched, warm or active
session. Suspended, expired and evicted sessions cannot renew. Expired sessions
cannot execute, change budgets/lifecycle, or restart with the same ID, even before
the cleanup worker runs. The originating valid login may still inspect and evict
them. These checks run again after the execution mutex is acquired; already
executing mutations are not rolled back.

## Reclamation

The private SQLite ownership row stores `lease_expires_at` in UTC epoch
milliseconds. Schema migration gives older rows deadline zero: no prior host
promised liveness, so old sessions fail closed instead of being silently revived.
Restart preserves the deadline and terminal ownership. It does not restore a
browser's live JavaScript state.

The server scans indexed batches of 128 pending ownership rows, advances past
live rows, and yields between full batches. It evicts expired runtime sessions
and retires ownership without requiring a surviving browser. Retired rows are
excluded from future sweeps. Terminal runtime metadata remains inspectable but
no longer contributes to reserved resource totals. These totals are declared
budgets, not measured process memory or proof of hard browser resource limits.

Cleanup normally runs every second between full passes; node lock contention or
storage failure can delay physical reclamation, but new expired-session requests
are denied at the boundary. The server relies on a correctly synchronized UTC
clock. Native in-process dispatch remains a trusted operator interface, outside
the HTTP login lease policy. Terminal-record/tombstone garbage collection is
still separate work.

## Browser Behavior

The Astro controller heartbeats immediately after start and before mounting. It
serializes renewals, validates session identity and timing fields, and measures
each deadline from the request's start, never its response arrival. A watchdog
stops local execution if renewal hangs or a lease expires. Renewal failure removes
the iframe and bridge synchronously, aborts renewal work, attempts remote eviction
and presents an explicit retry. Retry creates a new session; it never silently
remounts a game or pretends to restore unsaved state.

Renewal continues while the [document-bound bridge](surface-bridge.md) connects.
Warm/active transitions wait for confirmed channel readiness. Close, lease loss,
and account replacement cancel that wait; late confirmation cannot reactivate a
stopped opening. A legacy or failed handshake reports an explicit startup error.

Local elapsed time combines monotonic and wall-clock deltas without decreasing.
Forward clock corrections may conservatively end a lease early; backward ones
cannot extend it. This also accounts for OS sleep on browsers whose high-resolution
clock pauses during sleep, a documented [Performance.now limitation](https://developer.mozilla.org/en-US/docs/Web/API/Performance/now#ticking_during_sleep).
The host rechecks on visibility/pageshow and closes on pagehide. Abrupt crashes
need no unload request: the server deadline remains authoritative.

## Verification

- Ten real HTTP/RPC regressions cover renewal versus reads, expiry caps, login and
  Object isolation, suspended/terminal rejection, immediate expiry denial, idle
  reclamation, restart/migration, concurrent renewal, and fairness past live rows.
- Controller tests cover renewal without remounting, latency, malformed leases,
  cancellation, blocked startup, delayed timers, sleep, clock corrections and
  late results after replacement. SDK tests cover the host wrapper and rejection
  of heartbeat calls from embedded code.
- The real API/Astro/Aegis suite waits for an actual persisted renewal while
  retaining the same iframe, then expires only the disposable test-store lease.
  It verifies removal of executable content, server retirement, visible retry,
  and a fresh session. This is fault injection, not a simulated browser crash.
- `artifacts/surface-leases-api-host.trace.fozzy`,
  `artifacts/surface-leases-controller-final-host.trace.fozzy`, and
  `artifacts/live-stack-surface-leases-host.trace.fozzy` pass strict verification,
  replay and CI. Full Rust workspace tests, 201 frontend tests, 63 SDK tests,
  generated-contract checks, Astro check/build and changed-crate formatting pass.
  Scripted Fozzy doctor/test/fuzz scenarios verify orchestration only, not browser
  scheduling or algorithm fuzz coverage.

Lease cleanup closes abandonment, not checkpoint recovery. Explicit saved-state
restore, remote-code integrity/initial-redirect policy, complete game/browser-
capability workflows and hard resource enforcement remain in the production ledger.
