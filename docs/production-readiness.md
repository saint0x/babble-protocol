# Production Readiness

The active goal covers full social-platform functionality, including the
deprecated algorithm concepts and a complete Astro experience. Treat unverified
requirements as open. This ledger records current evidence and gaps; it is not
a completed audit.

## Current Priorities

The user clarified on September 30 that complete functionality and a gorgeous,
seamless social experience take priority over exhaustive production hardening.

1. Finish real user workflows: authoring and publication, post types, profiles,
   conversations, discovery, and interactive Objects with actual effects and
   visible outcomes. An inspector or action descriptor is not a finished feature.
2. Polish the approved rounded horizontal swipe-card system, smaller conversation
   cards, hierarchy, generous spacing, smooth transitions and complete interaction
   states. Use generated imagery where it improves relevant visual content, never
   as fabricated user activity or a substitute for functional media support.
3. Retain baseline safety and reliability: authentication, ownership, untrusted
   Object boundaries, meaningful consent, bounded inputs, durable user writes,
   and understandable failure/retry behavior.
4. Track advanced operational hardening separately. Large-scale load campaigns,
   deployment drills, extensive security features and independent audits should
   not displace missing social functionality or UX polish. Their absence must
   still be disclosed before a public deployment.

The historical evidence and broader hardening backlog below remain useful; they
are not all equal-priority blockers for the next product milestone.

## E2E Against Production Frontend Assets

September 30, 2026 (October 1 UTC): the separate full API/Astro/Aegis regression
passes against an isolated Astro production build and preview. The API still
runs through `cargo run`; this is not a release-binary or deployed-service test.

- Full evidence: `artifacts/browser-invocations/production-full-headroom.fozzy`,
  run `97d1a898-44a1-4064-b6c9-5e0b1792713f`, seed 930, 390.575 seconds.
  Strict trace verification, replay and all seven CI checks pass, independently
  verified. Replay checks recorded observations; it does not rerun the browser.
- Focused browser consent evidence: `artifacts/browser-invocations/production-focus-current.fozzy`,
  run `e0419713-9792-4ade-a097-a93f6e1ab867`, seed 930, 27.378 seconds, also
  passes strict verification, replay and all seven CI checks. Its manifest
  `artifacts/browser-invocations/live-focus-736ecf53-fd7d-4550-8ad8-f3a66d0e7085/`
  matches every one of the full run's 705 source hashes and current files.
- The full run covers publication/restart/retry, embedded document binding and
  leases, conversations, image/audio/video, media replies/shares and albums,
  profiles, public reactions, navigation/search, bundle authoring, permissions,
  browser and social invocation consent, private preferences/Following,
  mute/block and account security. It records 47 passing checkpoints.
- Compiled application HTML, JavaScript and CSS load successfully; development
  runtime and source URLs return 404. Application assets cannot reference the
  separately compiled test modules used by setup and component/layout probes.
  Consent registration is observed read-only in the disposable auth database
  and checked through the authenticated API, without patching application
  classes, transport responses or native browser APIs.
- All 705 recorded product/harness/fixture files match before, after and current
  source in `artifacts/browser-invocations/live-full-2e498fd8-7830-4368-bbf1-1963a10d3ccd/`.
  The harness reserves isolated service ports, awaits exact server readiness,
  and keeps its build output/cache separate from the normal preview. Dev and
  production isolation tests pass in `artifacts/astro-production-isolation-fixed-host.fozzy`,
  run `f70b0a68-b9e9-4a04-ae69-0da2f2f22163`, verified/replayed/CI.
  Both final runs' temporary stores were confirmed removed, the full run's child
  processes exited, and all three test service ports were released.
- A failing image-viewer probe was corrected to wait for the exact published
  Object's loaded, active card rather than selecting a retiring card during its
  exit transition. The original `production-full.fozzy` failure remains. A later
  12-second responsive timeout (`production-full-final.fozzy`) and disk-exhaustion
  failure (`production-full-diagnostic.fozzy`) are retained. After reclaiming
  rebuildable Rust test executables, the full run passed with disk headroom and
  unchanged timeouts; the earlier latency cause is not proven.

Native clipboard execution remains intentionally excluded. Aegis fullscreen
reported `CAPABILITY_UNAVAILABLE` with `trustedClick: false`; audio autoplay also
has a scripted-activation limitation. Component geometry and iframe widths are
not physical-device, touch, or visual-design approval. The existing abrupt
SIGINT/SIGTERM cleanup gap in `live-stack.mjs` remains open; ordinary completion
and assertion failures use its cleanup path. Other focused fixtures retain
development mode until individually migrated. Full platform and deployment
completion still require the open functionality and evidence listed below.

## Durable Browser Actions

September 30, 2026: clipboard and fullscreen now use v2 RPC methods, the shared
invocation journal, one-use dispatch tickets and typed host-reported outcomes.
The host keeps the rounded prompt, literal clipboard preview, default Cancel
focus and separate native-action click after authorization. Browser and social
requests share one permission slot per mounted Object. The [wire contract](invocation-consent.md#native-browser-methods)
states the retry, cancellation and native-activation limits.

- Focused client evidence: 88 tests (26 host actions, 18 browser API, 24 social
  invocations, six prompt and 14 permissions), recorded in
  `artifacts/browser-invocations/payload-budget-fixed-host.fozzy`, run
  `8b66907e-a947-40e3-978c-43e10c95291c`. Strict verification, replay and CI pass,
  including independent parent verification.
- The tests exposed and now protect against enum coercion, dispatch identity
  replacement, terminal-state resurrection and caller mutation during retries.
  An acknowledgement can retry identical bytes; a native effect cannot retry.
  Clipboard normalization now enforces the whole canonical and serialized JSON
  payload budgets, including framing and escaping. Boundary regressions cover
  ASCII, Unicode, quotes, backslashes, newlines and NUL.
- Scripted five-run doctor/strict tests pass but are not product execution.
  An eight-run scripted fuzz pass covers orchestration only; an earlier client
  fuzz attempt could not launch Node (`proc_unmatched`). Exploration rejected
  this non-distributed scenario. Neither establishes additional product coverage.
- Integrated client evidence: 178 SDK tests, 839 frontend tests, Astro check
  (zero errors/warnings, three nonblocking hints) and static build pass in
  `artifacts/browser-invocations/integrated-client-current-host.fozzy`, run
  `aa2fcc54-43a0-4fd6-84c9-db9653adccd2`, with strict verification, replay and CI.
  All 256 recorded client source/test/contract/output hashes remain unchanged.
  This includes the SDK schema-constant correction and generated discriminated
  union types. The earlier loader failure is retained separately; its attempted
  shrink drifted and is not a valid minimized reproduction.
- Backend evidence: 85 focused API/node/store/capability/RPC/runtime tests pass
  in `backend/crates/api/artifacts/invocation-consent/browser-focused-verified-host.fozzy`,
  run `85da6cc2-eb63-44cf-9433-5917183423b5`. Parent independently verified all
  27 source hashes plus strict trace verification, replay and CI. This is focused
  affected-contract coverage, not a completed full-workspace test run. The broader
  seven-package run was interrupted by host ENOSPC and has no completed trace;
  an unrelated media fixture teardown failure passed its isolated rerun. See the
  [backend handoff](../backend/crates/api/artifacts/invocation-consent/browser-handoff.md).
- Real Aegis acceptance against production frontend assets passes; see the
  full regression above. Native success in adapter tests is not physical-device
  proof, and browser tests must not overwrite the machine's clipboard.

Five external AskEachTime methods, sensitive AskOnce consent and the outstanding
algorithm integrations remain open. This milestone is not platform completion.

## Final Feed Selection

September 30, 2026: the personalized browser feed applies private filters and
preference scoring to a public pool of up to 200 candidates, reapplies canonical
source diversity, then selects nine cards. Only those cards are hydrated. The
inspector and feed metadata describe the final order rather than stale public
diversity decisions. Following remains chronological; a fully filtered pool stays
empty. Private preferences are not sent with public discovery requests.

- SDK evidence: 19 native fixture comparisons, 42 focused tests and 175 complete
  tests; `sdk/tests/diversity-sdk-protocol-host.trace.fozzy` passes strict trace
  verification, replay and CI.
- Frontend evidence: 805 tests, Astro check and build in
  `artifacts/final-feed-ui-host.fozzy`, run
  `2ae73cdd-dc04-4cee-854d-991ad5a529ef`, verified/replayed/CI.
- The real Python-worker relationship path now retains explicit public source
  and target text through local orchestration, with cache commitments binding
  both. The remote allowlist is unchanged. Full provider tests pass in
  `artifacts/relationship-pair-provider-host.fozzy`, run
  `401ed575-bad8-4614-aee1-98e84e1fdf05`, verified/replayed/CI. This fixes input
  preservation; it does not make the lexical relationship rule target-aware.
- Indexed retrieval now admits at most 200 Objects before expensive enrichment.
  The host-backed node/discovery/graph/store and Python-provider regression passes
  293 tests in `artifacts/indexed-discovery/final.fozzy`, run
  `29c5da93-5d3b-4978-bbc8-649f25296327`. Parent review independently verified the
  source manifest and strict trace verification/replay/CI. Tests cover 256/1,024
  Object corpora, older active content, full overlapping source provenance,
  lexical parity, failed writes, restart and moderation reversal.
- Focused real API/Astro/Aegis acceptance passes in
  `artifacts/feed-diversity-browser/focused-host.fozzy`, run
  `e1eab989-d44f-481f-994c-c67e6a95116b`, independently verified/replayed/CI.
  Forty-two signed posts and four signed graph edges exercise actual Settings
  controls, hiding 30 posts while refilling nine cards, wire privacy, selected-only
  hydration, swipe positions, final inspector reasons, distinct Lenses, empty
  search and unchanged chronological Following. Graph-anchor source checks run
  separately at the API boundary; the actual UI does not invent anchors.
- The separate full API/Astro/Aegis regression also passes in
  `artifacts/feed-diversity-browser/full-host.fozzy`, run
  `b2658e94-42b4-46b5-acd3-30153a1f545f` (357.047 seconds), independently
  verified/replayed/CI. It covers media, publication, profiles, conversations,
  Following, reactions, private safety, account security and embedded Object
  lifecycle/consent alongside the updated feed. The focused test above owns
  final-diversity acceptance; neither test establishes semantic model quality.
- All 438 recorded product/harness/fixture files remain unchanged across browser
  acceptance. The normal API runs this build against the original store; health
  and ranked discovery pass and the complete saved public identity is unchanged.
- Final Aegis inspection of port 4321 shows Online, nine Objects, 32px primary
  card corners, loaded host-action/moderation styles and no Astro/Vite overlay.
  The missing `host-actions.css` error is no longer present. Disposable browser
  test services/stores are removed; the normal preview remains running.

Diversity floors remain soft adjustments based on primary source, not hard
quotas or a guarantee of sources absent from the admitted pool. The outstanding
semantic definitions and actual model-service requirements are recorded in
[the semantic integration audit](semantic-integration.md).

## Reporting And Appeals

September 30, 2026: private Object reports, explicitly configured reviewer queues,
policy-versioned decisions, author notices and independent appeals are integrated
across the backend and Astro client. Focused real API/Astro/Aegis acceptance and
the separate combined regression pass. See [the contract](moderation.md).

- Reports alone do not restrict content. Reviewed restrictions remove Objects
  from discovery, Following and reply projections, redact quoted content while
  preserving signed relationships, and stop local Object execution. Appeals stay
  restricted until independently resolved. Multiple cases compose correctly.
- Post and reply controls target the actual Object. The account menu opens private
  reports, affected-author notices and, only for configured reviewers, the queue.
  Current server authority, exact retry receipts and revision checks remain
  authoritative. Registration never grants review authority.
- Backend evidence: 26 focused tests and 81 adjacent regression tests recorded in
  `backend/artifacts/moderation/final-host.fozzy` and
  `backend/artifacts/moderation/regression-final-host.fozzy`. Source checksums and
  strict trace verification, replay and CI pass.
- Complete frontend tests, Astro check and build pass in
  `artifacts/moderation-ui-host.fozzy`, run
  `beb225c0-696d-4811-bade-18cb9266e510`. SDK generation checks, build and 146 tests
  pass in `artifacts/moderation-sdk-host.fozzy`, run
  `f5f5558d-c307-4192-8d75-984a631b8bff`. Both traces verify/replay/pass CI;
  independent Python fixture conformance passes.
- Real browser acceptance passes in `artifacts/moderation-browser/live-host.fozzy`,
  run `69ae0813-b733-48ab-b5a6-b91925d785ac`, verified/replayed/CI. It covers UI
  submission, cancellation/focus, role separation, decisions, independent appeals,
  retry/CAS, restart and runtime enforcement, and same-page child comment/quote
  removal and restoration. Responsive production-page geometry passes at
  320/390/1280; this is not physical-touch or screenshot approval.
- Independent review caught unrelated private reports invalidating Following
  cursors; snapshots now depend on effective restrictions only. A separate SDK
  test fix synchronizes virtual timers and deadline clocks without changing
  production timeout behavior.
- The separate complete live-stack regression passes in
  `artifacts/moderation-browser/full-stack-host.fozzy`, run
  `3a1baec1-93bb-4b3b-9758-4db850ccf2ad` (361.676 seconds), with strict trace
  verification, replay and CI passing. It covers existing social/media/account
  and Surface journeys against the integrated source; moderation's role journey
  remains the separate focused run above. The 198-file client/fixture/harness
  source manifest verifies unchanged. Test processes and stores were cleaned up.
- The normal API now runs the verified build against the original store. Its
  health is good and the saved `deepsaint` public identity is unchanged.
  Final Aegis inspection shows the preview Online with nine Objects, loaded
  host-action/moderation styles, 32px rounded primary cards and no error overlay.

These workflows do not establish calibrated semantic moderation, public blob
removal, federation-wide bans, or completion of the broader algorithm and external
executor work. Intake is capped at 1,000 reports per account on this node; existing
reviews, appeals and retries remain available. No reviewer was automatically
configured for the saved development account.

## Private Social Safety

September 30, 2026: account-level mute and block are implemented across signed
private storage, authenticated host REST, generated contracts, and the Astro UI.
Focused real browser acceptance and the combined full-stack regression pass.
This is not a claim of complete platform readiness. See
[the contract](social-safety.md).

- Author controls are available from cards and profiles; the account menu has a
  Blocked and muted management dialog. Block confirmation states the local-node
  scope and does not imply that public posts become private.
- Mute filters recommendations and Following. Block additionally prevents new
  local-node follows, replies, shares, reactions and graph relationships in
  either direction, while permitting legitimate withdrawals and preserving
  historical signed data. Blocked reply rows and quote previews are omitted.
- Account snapshots are private, authenticated and no-store. Durable signed
  receipts bind exact intent and revision. Following cursors include the safety
  revision. Records never enter public exports or Judgment inputs.
- The host gates feed requests on current-account state, waits for superseding
  refreshes, clears unsafe caches on failure, and reloads after recovery even if
  the revision is unchanged. Account changes invalidate private context. Explicit
  profile/Object navigation is preserved.
- Backend evidence: 26 focused tests and 495 affected graph/store/node/API/schema
  tests, final host run `e346c46d-5d91-43e3-9da1-d92caa3dd9c5` in
  `backend/crates/node/tests/artifacts/safety/final-regression-host.fozzy`.
  Parent verified the source hashes, trace, replay and CI.
- Host evidence: 753 frontend tests, Astro check and production build in
  `artifacts/social-safety-ui-transitions-host.fozzy`, run
  `9a7a3d99-4472-4a01-a922-0dfbfe948c79`; strict verification, replay and CI pass.
  All 145 SDK tests and independent fixture conformance pass separately.
- Early real-browser runs caught a malformed test Object fixture and a real
  rapid-toggle transition defect. The fixture uses canonical metadata now;
  closing panels reverse their exit timer on the next toggle. Assertions were
  preserved. The focused real API/Astro/Aegis run now passes in
  `artifacts/social-safety-browser-verified-host.1.fozzy`, run
  `b499389e-aeeb-4b37-8aad-ec90f916487c`, verified/replayed/CI. It covers UI
  entry points, four-lens filtering, explicit navigation, preview filtering,
  bidirectional write rejection, restoration, CAS/replay and owner isolation.
  Production-page iframe geometry passes at 320/390/1280 with 44px controls;
  this is not physical-device or screenshot approval. Public export exclusion
  remains backend evidence.
- The complete live API/Astro/Aegis regression passes in
  `artifacts/live-stack-social-safety-host.fozzy`, run
  `4a1cb334-4827-4899-ab8a-80d037c51b01` (316 seconds), verified/replayed/CI.
  It includes safety alongside real publication, media albums/replies/shares,
  profiles, conversations, Following, reactions, preferences, account security,
  signed Surface lifecycle, permissions and one-use social invocation consent.
  Final host sources are recorded in `artifacts/social-safety-source.sha256`.
- The normal API was restarted with the new build against the existing store.
  Health is good and the complete saved `deepsaint` public identity is unchanged.
  Aegis confirms port 4321 is online with nine Objects, 32px rounded cards, loaded
  safety styles and controls, and no Astro/Vite overlay.

Reporting, operator review, policy-versioned enforcement and appeals are now
covered by the newer milestone above. Personal safety remains separate from
those workflows, semantic moderation, the remaining algorithm migrations, and
the other external Object executors.

## Social Invocation Milestone

September 30, 2026 status: **four social actions verified through real
API/Astro/Aegis acceptance**. Older sections and traces describe their recorded source
milestones, not verification of this newer behavior.

- SDK follow/unfollow/share/reply helpers now call `.v2`; the corresponding
  `.v1` mutations return `UnsupportedVersion` (`UNSUPPORTED_VERSION`). Capability
  declarations stay at version 1. No durable social approval grants are issued
  or accepted as invocation authority. Supported social declarations can be
  promptable at admission while other policy and integrity gates remain intact.
- Authenticated actor/login, Object/version, document/session, normalized intent,
  policy and deadline bind each challenge. The node commits one-use consumption,
  aggregate quota accounting, signed social effects and `PublicationReceipt`
  together. Exact retries reuse the outcome; changed intent conflicts.
- Production `main.ts` uses the dedicated host prompt/controller for Surface
  calls, with explicit Allow Once/Deny/cancel, exact intent validation, lifecycle
  cancellation and suppression of late output. Completed cancellation
  reconciliation returns a result only to a still-live requester. Permission
  management shows Ask every time rather than a reusable social Allow.
- Host composer actions use explicit host documents and intent-derived request
  keys. Backend and frontend now integrate read-only completed-result recovery
  for the original actor/login after document expiry or retirement. Recovery
  neither renews execution authority nor causes a new effect.

### Reported Integration Evidence

These are scoped owner reports, not a new verification run by the documentation
refresh and not evidence of browser acceptance:

- Parent reports 61 focused frontend tests passing, including exact canonical
  byte comparison and live-only completed cancellation reconciliation regressions.
- API owner reports the final recovery-inclusive API/runtime/RPC run passing
  262 tests, including 135 API library tests (ten invocation tests), 28 auth
  integration tests, other API integration suites, seven RPC and 22 runtime tests.
  Real host trace
  `backend/crates/api/artifacts/invocation-consent/verified-recovery-host.fozzy`,
  run `5d46dd6d-6a30-45c0-a46b-a2d1c8d59311`, passes strict trace verification,
  replay and CI. This is backend evidence, not production browser acceptance.
- Parent also reports 226 node/store/capabilities tests passing, including 16
  invocation tests, with recorded host traces verified, replayed and passing CI.
- The final full client host run passes tests, Astro check and production build:
  `artifacts/invocation-client-recovery-final-host.fozzy`, run
  `bce05051-43c7-4641-84a6-31f2d6d7a820`, with strict trace verification, replay
  and CI passing. Standard-library conformance also passes after replacing the
  v1-only assumption with explicit metadata-version matching and required social
  v2 methods. These client checks do not establish real browser acceptance.
- Earlier API integration evidence covered 133 API, seven RPC and eleven runtime
  tests with verified/replayed/CI host trace
  `backend/crates/api/artifacts/invocation-consent/verified-host.fozzy`. That report
  preceded the recovery addition and is not final-source recovery evidence.

### Real Browser Evidence

The signed-bundle API/Astro/Aegis run `e28871bf-bf22-4ccd-b02d-3fd37fe56738`
passes, recorded in `artifacts/invocation-consent-live-layout-host.fozzy`.
Strict trace verification, replay and CI pass. It verifies all four social
actions against backend Object/edge readback, one-effect exact retries, immutable
literal preview, denial/cancellation/expiry/closure without writes, and rejection
of wrong-document, other-actor and anonymous access. Host focus and event
isolation pass. Production-CSS dialog snapshots fit 320/390/1280 widths; this
does not establish physical-device gestures or complete journeys at each width.
The first browser attempt failed on an Aegis primitive-value test probe; the
probe now returns an object and the recorded rerun passes without weakened
application assertions. Same-actor/different-login, media and restart coverage
exists at the backend layer but is not part of this focused embedded-app run.

The complete API/Astro/Aegis regression also passes, run
`c64c478b-05dc-48c0-ab89-f33f4b5c8aaa`, recorded in
`artifacts/live-stack-social-invocations-host.fozzy` (267 seconds). Strict trace
verification, replay and CI pass. This includes the new invocation flow alongside
real media reply/share/albums, profiles, reactions, conversations, following,
permissions, native host-action confirmation, account/session workflows, signed
Surface lifecycle and the existing responsive rounded-card assertions.
Replay validates recorded observations; it does not rerun the live services.
The strict five-run doctor/test and 32-run client scenario fuzz checks use
scripted process contracts, distinct from these real host-backed executions.

The normal API at 8787 now runs this build against the existing `.babel-node`
store. The saved `deepsaint` identity is unchanged. Aegis confirms the preview at
4321 remains online with nine Objects, its 32px rounded cards, both host-action
stylesheets loaded, and no Astro/Vite error overlay. No account reset occurred.

At this social milestone, seven `ask_each_time` methods remained outside durable consent:
`babel.payments.checkout.v1`, `babel.ai.generate.v1`, `babel.ai.transcribe.v1`,
`babel.media.camera.request.v1`, `babel.media.microphone.request.v1`,
`babel.clipboard.write.v1`, and `babel.fullscreen.enter.v1`. Raw RPCs still return
action descriptors; the browser confirmation at that point was not durable
invocation enforcement. The later browser integration migrates clipboard and
fullscreen to v2, leaving five external methods. Sensitive AskOnce migration
and retention/GC remain open. See the current
[invocation contract](invocation-consent.md) for exact wire behavior and scope.

## Verified Conversation Milestone

- Implemented horizontal Object navigation with independent vertical conversation
  columns, media-specific primary cards, real reply pagination, nested replies,
  retry states, and preserved reading position.
- Verified publication through backend readback and the rendered thread. Unit
  coverage additionally checks a newly published reply after earlier pages load.
- Retained the grayscale design language, spacing, and reduced-motion behavior.
- Full Rust workspace tests, Astro checking/build, and 31 frontend regression
  tests pass. The real Aegis/API/Astro run checks nested Back and cross-post scroll
  restoration. Its trace `artifacts/live-stack-conversations-nested-host.trace.fozzy`
  passes strict verification, replay, and CI.

## Verified Public Ranking Milestone

- Default API discovery now invokes typed Python for all eight canonical public
  lenses, weighted numeric traces and soft source diversity. Public candidate
  admission is capped independently of output count and retains overlapping
  provenance. See [the ranking boundary](public-ranking.md).
- Nineteen native/Python golden cases, full 200-candidate/all-lens transport,
  in-memory float round-trip, malformed output, restart/deadlines, shared-worker
  lifetime, API provenance/503 behavior and private Following isolation pass.
  Correctly rounded JSON parsing preserves exact echoed candidate identity;
  Judgment frames retain their independent byte/node limits.
- The live suite caught a confirmed image publication disappearing when it fell
  outside ranked output. The UI now directly opens the acknowledged public post
  without inventing ranking provenance. Stale completions cannot redirect it,
  and private Following never receives this public insertion.
- Rust workspace: 308 tests. Python: 233 tests plus strict type/lint checks.
  Frontend: 107 tests and Astro check/build (one existing async hint). SDK: 46
  tests and current generated contracts. Ranking/worker/discovery Clippy passes;
  broader warnings-as-errors remains blocked by five existing node lints
  (`needless_borrow` and four `too_many_arguments` cases).
- `artifacts/public-ranking-final-host.trace.fozzy` and
  `artifacts/live-stack-public-ranking-verified-host.trace.fozzy` pass strict
  verification, replay and CI. The latter executes actual API/Astro/Aegis flows
  including Python ranking readback, social/media/Surface operations and rounded
  card geometry. The earlier failed live trace is retained as failure evidence.
  Scripted Fozzy doctor/test/fuzz checks only orchestration, not real algorithm fuzzing.

## Verified Host Lease Milestone

- Authenticated HTTP sessions now have persisted 60-second host leases capped by
  login expiry. Only the originating host login can renew; expired sessions
  cannot execute or revive, even before bounded cleanup. Reads and embedded
  calls do not extend deadlines. Migration fails closed for pre-lease sessions.
- Frontend renewal preserves the mounted iframe. Failure/expiry tears down local
  execution and exposes explicit fresh-session retry; watchdogs cover delayed
  responses, clock corrections and sleep. Native health totals exclude evicted
  reservations while retaining inspectable terminal metadata.
- Full Rust workspace, 201 frontend and 63 SDK tests, generated contracts,
  Astro check/build, and changed-crate formatting pass. Real API/Astro/Aegis
  renewal, injected expiry and retry pass alongside existing social workflows.
  API, controller and live-stack traces pass verification/replay/CI; see
  [the lease contract](surface-leases.md). Saved-state recovery, actual browser
  resource enforcement, and terminal-history garbage collection remain open.

## Remaining Gaps

The [source-agreement milestone](source-agreement.md) now covers the seventh
Python Judgment, signed public-source selection, copied-text/role deduplication,
finite bounded inputs, atomic request/result persistence, restart reuse, and
on-demand UI evaluation with exact input disclosure. This does not close full
algorithm migration or whole-platform production readiness.

The inline Surface milestone is recorded in [its implementation contract](inline-surfaces.md):
159 frontend tests, 62 SDK tests, Astro check/build, and a verified real
API/Astro/Aegis trace. Full Rust workspace tests also pass. API-only formatting
passes; full-workspace formatting still reports an unrelated graph-module
re-export wrapping difference. These checks do not close the gaps below.

| Requirement | Current Evidence | Completion Evidence Needed |
| --- | --- | --- |
| Account lifecycle and deployment | Authenticated ownership, durable sessions/signing keys, viewer-specific grants, private state, and operator separation have regression coverage. Account settings now expose password change and active-session revocation with restart and browser evidence. | Account recovery, MFA, compromised-password screening, security activity/alerts, deployment TLS/abuse controls, backup/recovery drills, and independent security review. Sessions do not claim device identification. |
| Persistent reactions and certainty | [Public reactions](reactions.md) now have independent axes, signed atomic persistence, authenticated REST/RPC, durable retries, explicit public consent, conflict recovery, withdrawal, and real browser readback. | Capability-mediated Surface consent/subscriptions, rotated-key public proof bundles, federation semantics, retention/load/backup evidence. Counts are not automatically ranking or Judgment inputs. |
| Complete social workflows | Public profiles have authoritative identity and bounded authored-Object pagination. Private person-following has signed transactions, durable retries, follow/unfollow controls, a private people list, and a chronological Following feed. Public reactions have a separate consent and persistence boundary. | Broader social workflow coverage, operational retention/load/recovery evidence, and physical-device/accessibility review. |
| Executable object experience | Inline rounded-card hosting, expansion without remounting, exit/restoration, eviction and fresh-session reopen pass real API/Aegis checks. [Host leases](surface-leases.md) bound abandoned HTTP sessions. The [document-bound bridge](surface-bridge.md) admits a single child-created port and waits for readiness before activation. | Complete game/host-action workflows, checkpoint restoration, remote-code integrity and initial-redirect policy, terminal-history retention policy, and all supported object kinds. |
| Protocol coverage in the UI | A method catalog and inspector expose backend information but do not constitute a complete workflow. | A method-to-user-workflow map with real entry, outcome, recovery, and accessibility evidence. |
| Full algorithm migration | Python executes seven Judgments, canonical public Lens ranking/diversity, and temporal scoring. Source agreement has a live signed-evidence consumer. Indexed retrieval bounds provider preparation to 200; browser filtering/scoring reapplies diversity before selecting nine. See the newer evidence above. | Engagement/private recommendation consumers, source-independence and semantic evaluation, empirical retrieval/temporal/diversity policy quality, named semantic definitions, calibrated models, and the remaining migration matrix. |
| Atomic publication and recovery | Bounded redo publication covers Objects, events, Judgments, social/provenance edges, ten publication RPCs, and consent grant/revoke retry receipts, with process-exit/restart tests. API readers gate on recovery readiness. | Retry/transactions for remaining mutation families, receipt retention/GC policy, deployment recovery drills, and the documented single-writer restriction. |
| Rendered design validation | Aegis supports DOM and geometry checks; the installed API has no screenshot or native viewport sizing command. | Visual review and physical-device touch evidence, particularly nested scrolling and embedded applications. Respect the user's Aegis-only constraint. |

The API defaults to loopback. Authenticated ownership is not a complete public
deployment: use one serving node per store and keep the store private. Native
Rust dispatch remains a trusted operator API. Do not expose the preview publicly
as a workaround for deployment work.

## Verified Account Milestone

- Backend workspace: 197 tests pass, including authenticated HTTP and imported
  cross-actor grant-revocation regressions. The backend host trace
  `backend/artifacts/auth-complete.fozzy` passed verification, replay and CI.
- SDK: 46 tests pass, including attempts by embedded Surfaces to call host-only
  signing, consent, and administration methods. Generated contracts match.
- Frontend: 44 tests pass; Astro checking/build pass with one existing async
  suggestion. Session tokens are host-only; local history is account-partitioned.
- The actual API/Astro/Aegis run verifies registration, impersonation denial,
  logout/login/restoration, executable Surface hosting, follow/reply/share,
  nested conversation return, and image publication/rendering. Captionless media
  no longer exposes serialized payloads as its caption.
- `artifacts/live-stack-authenticated-columns-host.trace.fozzy` passed strict
  verification, replay and CI. Earlier failed traces remain as failure evidence;
  this trace covers the corrected authenticated workflow.
- The [algorithm audit](algorithm-migration.md) covers all 14 deprecated modules
  and 96 functions. The 44-test typed Python package has stronger feedback,
  finite-value, consensus and discovery handling. Runtime consumer integration
  and several semantic requirements remain missing; passing unit tests do not
  close those requirements.

## Verified Python Execution Milestone

- The API defaults to a persistent local `babel-python/lexical-v1/1` worker.
  Real health exchange precedes listening; startup fails without the installed
  package. Explicit `rust-local` mode is available, never a silent fallback.
- Versioned Rust-owned DTOs, exported schemas/fixtures, strict Python wire
  validation, response validation, parameter-bound commitments, deadline/restart
  behavior, minimal child environment, and resource limits are implemented.
  See [the worker contract](algorithm-worker.md) for platform-specific limitations.
- Publication preflights all ingestion Judgments before durable Object/event
  writes. Node regressions cover provider failures and invalid responses across
  publication variants. This earlier milestone did not close transaction recovery;
  the bounded publication follow-up below adds that storage boundary.
- API work is bounded and moved off the async reactor. Tests cover saturation,
  client disconnect while mutation continues, oversized bodies, and independent
  reactor progress. Throughput under representative multi-user load is unproven.
- The full Rust workspace test suite passes. Python: 155 tests plus configured
  strict basedpyright and ruff pass. SDK: 46 tests and generated-contract checks
  pass. Frontend: 45 tests and Astro checking/build pass (one existing async hint).
- The actual API/Astro/Aegis suite verifies all six Python definitions, automatic
  publication analysis, repeated evaluations, provider provenance, and honest
  confidence labels, alongside authenticated social/media/Surface workflows.
  `artifacts/live-stack-python-verified-host.trace.fozzy` passed strict verification,
  replay, and CI. Strict scripted doctor/test preceded this host run; scripts alone
  are not actual execution evidence.
- The broader strict Clippy run at this milestone stopped on the existing
  `manual_is_multiple_of` lint in the realtime crate. A follow-up replaced that
  expression with the standard integer method; all six realtime tests and that
  crate's strict Clippy gate pass. The latest whole-workspace strict Clippy run
  stops on the existing `items_after_test_module` lint in `babel-crypto`; node
  also has existing argument-count/test-clone warnings. No blanket allowances
  were added to claim a clean workspace gate.
  A root-directory basedpyright attempt
  also incorrectly scanned the deprecated tree; the corrected invocation from
  `algorithms/` loads the package's checked-in strict configuration and passes.

These outputs remain lexical. The UI calls them heuristic or uncalibrated; neither
the worker nor its passing transport tests supplies a semantic accuracy claim.

## Publication And Draft Follow-Up

- Bounded redo publication commits Objects, signed events, ingestion Judgments,
  and associated reply/share/fork/remix edges together. Providers run first;
  restart validates and rolls forward committed journals before rebuilding indexes.
  See `../backend/crates/store/PUBLICATION.md` for limits and unsupported cases.
- Store/node evidence includes 54 native tests and actual child-process exits
  at 17 phases. The API's shared node-lock helper now rejects recovery-required
  handles before infallible getters can expose an old snapshot. A real installation
  failure regression covers RPC and REST rejection followed by restart recovery.
- Drafts are isolated by API origin, account, mode, and parent Object. Late
  completions cannot clear newer edits or redirect a newer composer. Text persists
  locally; guest drafts and media Files are memory-only. Twenty-one focused draft
  tests pass as part of 73 frontend tests, including account changes during the
  composer's closing animation. The retry follow-up below adds durable server
  deduplication for the Object/social publication operations.
- The full Rust workspace tests pass with `CARGO_INCREMENTAL=0`. The first attempt
  exhausted disk during linking; only this project's rebuildable incremental cache
  was cleared before rerunning. Astro check/build pass. The rounded-card restoration
  has an independently verified/replayed/CI-checked host trace at
  `artifacts/card-style-responsive-host.trace.fozzy`, including responsive DOM
  geometry and anchored conversation scrolling. This is not screenshot approval.
- The integrated API/Python/Astro/Aegis run now passes with this publication,
  draft, reading-anchor, and rounded-card revision. It exercises actual account
  workflows, draft A/B and reply/share/publish isolation, signed publication,
  nested conversations, image readback, and executable Surfaces.
  `artifacts/live-stack-rounded-cards-verified-host.trace.fozzy` passes strict
  trace verification, replay, and CI. Earlier failures exposed the composer
  account-status race and the harness's handling of omitted Aegis null results;
  those failures are preserved, not counted as passes. Avoid running Astro check
  or build concurrently with this suite: both use the same managed dev root.
  SDK tests also rebuild the imported SDK and can trigger a live-page reload;
  complete them before the browser suite.

## Public Profiles And Retry Follow-Up

- Public profiles are read-only views of an authoritative identity and its signed
  Objects, with bounded pages, newest-first tie ordering, restart-stable cursors,
  and explicit refresh on snapshot change (including backdated imports). Profile
  selection uses the existing rounded swipe deck and retains original feed reading
  position. This does not implement author-following or profile editing.
- Durable receipts cover ten Object/social publication RPCs. Key reuse with a
  different method, payload, or grant set conflicts; equivalent grant ordering
  does not. HTTP ownership, sessions, and current grants are checked before replay.
  The API tests cover concurrency, restart, installation failure, logout/relogin,
  impersonation, malformed receipts, missing edges, and substituted outcomes.
- Store error/process-exit coverage is now 19 phases with receipt installation.
  The real host trace `artifacts/publication-retry-validated-host.trace.fozzy`
  passes strict verification, replay, and CI. It executes Rust tests; the separate
  strict doctor/test and 30-run scripted property fuzz are orchestration checks
  only. Fozzy distributed exploration is inapplicable to this steps scenario;
  process-exit fault coverage is in the native storage tests.
- Full Rust workspace tests, 46 SDK tests, 83 frontend tests, Astro check and
  build pass. These checks do not constitute physical-device or screenshot review.
- The real Python-backed API/Astro/Aegis run now covers publication replay before
  and after process restart, public profile readback, post/reply-author profile
  entry, own profile versus account settings, selected-post navigation, and feed
  position/focus restoration. Desktop plus 390px/320px same-origin frames confirm
  rounded primary cards, narrower replies, visible neighbor cards, and no horizontal
  overflow. `artifacts/live-stack-profiles-retries-verified-host.trace.fozzy`
  passes strict verification, replay, and CI. An earlier run caught the test
  checking `hidden` before the composer's closing animation ended; the corrected
  test waits for that observable end state. The failed trace is retained.

## Person-Following Follow-Up

- [Person-following](following.md) is account-private and separate from public
  Object-follow edges. Authenticated REST infers the acting principal; private
  records never enter discovery or public event bundles. Signed SQLite actions,
  materialized state, snapshot versions, and retry receipts commit together.
- Revision checks, stable retry keys, restart recovery, receipt-chain integrity,
  concurrent changes, signing-key rotation/expiry, and backward-clock rejection
  have native coverage. Transaction-time signer validation and timestamp checks
  prevent a successful write from making the next reopen fail. Receipt deletion
  is detected against action commitments and per-author receipt chains/heads;
  whole-store rollback still has no external integrity anchor.
- The chronological feed merges per-author indexes, binds cursors to viewer and
  snapshot, searches complete text, and bounds scan/output work. Empty filtered
  pages can continue. Follow changes and new or backdated Objects invalidate
  snapshots; repeated imports do not. Public profiles share the output byte
  bound. The frontend handles retry, conflict refresh, pagination, account
  changes, and self/guest states without falling back to discovery.
- Full Rust workspace tests, 46 SDK tests, 104 frontend tests, independent fixture
  conformance, Astro check, and Astro build pass. Astro reports one existing
  optional async-conversion hint. Focused native/API/frontend Fozzy host traces
  are retained alongside strict orchestration checks; scripted checks alone do
  not prove application behavior.
- The real Python-backed API/Astro/Aegis run passes author-profile entry, durable
  follow readback, 21-post chronological pagination, the private people list,
  unfollow, empty-state recovery, and hidden self-follow controls. Existing
  account, draft, reply, media, Surface, profile, and rounded-card checks also
  pass. `artifacts/live-stack-following-verified-host.trace.fozzy` passes strict
  verification, replay, and CI (run `f38a35ed-edda-43d8-977b-aa9aa831e5b4`).
- Earlier failed runs remain recorded. The harness now uses visible settings
  controls, navigates mixed discovery results instead of assuming an author is
  ranked first, and waits for native dialog visibility before clicking. The
  obsolete Object-follow menu check was replaced by the person-following journey;
  native Object-follow capability tests remain intact.
- Rounded 28px primary cards, smaller reply columns, tonal backgrounds, and
  visible neighbor cards pass desktop and 390px/320px frame geometry checks.
  This is not screenshot approval or physical-device touch evidence. The broader
  platform goal remains open, including indefinite history retention, deployment
  recovery/load testing, reactions, algorithm semantics, and complete UI coverage.

## Public Reactions Follow-Up

- Three independent reaction axes and optional position confidence now persist
  as public, signed identity/Object registers. CAS prevents lost updates; durable
  retry receipts preserve exact acknowledgments across restart without restoring
  an old value as current state. Withdrawal removes current counts, not history.
- Atomic first installation, transaction rollback, startup integrity, key expiry
  and rotation, and live receipt tampering have focused native coverage. The
  independent audit caught unchecked receipt replay; replay now verifies the
  historical signature, indexed identity/hash, predecessor, and signed action.
- The card keeps compact appreciation controls and discloses the other axes.
  First publication requires explicit public-consent confirmation. Account
  changes invalidate pending work; conflicts refresh and require a new choice.
- Verification: the full Rust workspace, 222 frontend tests, 65 SDK tests, canonical schema/fixture
  conformance, Astro check/build, 13 API regressions, and 15 focused store/node
  regressions. The real API/Python/Astro/Aegis suite verifies restart, public
  consent, confidence zero, competing-tab conflicts, withdrawal, and layouts
  from 320px to 1440px alongside existing platform workflows.
- Verified/replayed/CI-passed traces:
  `artifacts/reactions-live-receipt-integrity-verified-host.trace.fozzy`,
  `artifacts/reactions-api-host.trace.fozzy`,
  `artifacts/reactions-client-final-host.trace.fozzy`, and
  `artifacts/live-stack-reactions-final-host.trace.fozzy`.
  Scripted doctor5/test/30-run fuzz checks orchestration, not actual input fuzzing.
- Earlier failures remain recorded. One browser run was disrupted by a concurrent
  SDK rebuild; another exposed a geometry assertion that ignored the independent
  column's reading offset. The clean final run passes. PATH's FozzyLang wrapper
  also wrote incompatible trace checksums; the explicit determinism-engine path
  produced the verified traces. Shrinking the failed integration trace did not
  produce a useful smaller reproduction.
- These milestones do not close the full production audit, indefinite history
  retention, external rollback anchoring, or physical-device/visual review.

## Account Security Follow-Up

- Host-only account endpoints list active sessions using independent opaque
  management IDs, never bearer tokens or hashes. Owners can revoke one session
  or all other sessions. Legacy migration preserves existing logins and marks
  unknown creation times as unknown rather than fabricating them.
- Password change verifies the current password, then atomically updates its
  hash and revokes every session. Concurrent login/change operations compare
  the observed hash before committing. New passwords use a 15-Unicode-scalar
  minimum and 1024-UTF-8-byte maximum without normalization or truncation;
  legacy login verification remains compatible.
- Account settings expose these operations with confirmations, current-login
  markers, refresh/error states, secret clearing, and account-switch isolation.
  Lost password-change acknowledgments are explicitly uncertain and never
  trigger automatic retries. Canonical schemas include the account DTOs.
- Verification: full Rust workspace, 89 API library tests, 16 account-security
  API integration tests, 238 frontend tests, 65 SDK tests, independent fixture
  conformance, and Astro check/build pass. Astro retains two async-conversion
  hints. Strict Clippy remains blocked by existing node/RPC diagnostics.
- The real API/Python/Astro/Aegis run verifies password changes, rejected old
  credentials, individual/current/all-other revocation, cancellation, restart
  persistence, and 320px/390px account-dialog containment. Existing publishing,
  Surface, profile, following, reactions, and rounded-card journeys also pass.
  Primary cards retain 28px corners and replies occupy approximately 85-88% of
  their width across checked layouts from 320px to 1440px. Frame geometry is not
  screenshot approval or physical-device touch/accessibility evidence.
- Strictly verified, replayed, and CI-passed host traces:
  `artifacts/account-security-native-host.trace.fozzy`,
  `artifacts/account-security-api-host.trace.fozzy`,
  `artifacts/account-security-client-host.trace.fozzy`, and
  `artifacts/live-stack-account-security-host.trace.fozzy` (live run
  `84a30167-354d-4696-ae04-29075bb4753e`). Saved client scenarios are
  `tests/account-security-client.fozzy.json` and its `-host` companion.
  Scripted doctor5/strict-test/fuzz checks prove orchestration, not application
  execution; the host traces supply actual execution evidence.
- Revoked tokens immediately lose authorization. Already-mounted Surfaces in
  other browsers stop on heartbeat/watchdog failure, not a claimed instantaneous
  remote teardown. Recovery, MFA, compromised-password screening, public-ingress
  security, activity alerts, and the broader production audit remain open.

## Temporal And Search Integration

- Public discovery now consumes the typed Python temporal scorer using one real
  evaluation time per request. Native replay accepts an explicit clock. Content
  classification reads content fields, not serialized schema metadata; activity
  uses eligible signed public relationships, never private reading/follow data.
- Canonical Rust/Python/SDK contracts preserve input identity, timestamp
  precision, provider/version provenance, bounded batches, and validated finite
  outputs. Python failure is explicit, not a silent native fallback. Analytics
  and inspection expose the actual returned components and evaluation time.
- Explicit search stays within its lexical matches. Anchors, graph expansion,
  and exploration cannot inject unrelated Objects into a search. Blank discovery
  retains graph expansion. Empty discovery no longer triggers a raw-search
  fallback in the frontend or loses its policy/local-personalization context.
- Verification: full Rust workspace; 485 Python, 248 frontend, and 65 SDK tests;
  Python typing/lint; generated SDK checks; independent fixture conformance;
  Astro check/build. Nine node temporal, three temporal API, and four search API
  integration tests exercise real Python workers and private Following isolation.
  Astro retains two existing async-conversion hints.
- Strictly verified/replayed/CI-passed host traces are listed in
  `temporal-scoring.md`. Initial failures caught stale tests that expected an
  explicit search to include unrelated graph neighbors; separate blank-discovery
  and matching-only search assertions now preserve both contracts.
- The complete API/Python/Astro/Aegis run passes in
  `artifacts/rounded-cards-reading-restored-host.trace.fozzy`, with strict trace
  verification, replay, and CI passing. It verifies visible temporal analytics,
  empty-search recovery, preserved privacy boundaries, and existing account,
  publishing, reaction, profile, Following, conversation, and Surface journeys.
  The run also reproduced and fixed shared reaction controls collapsing an
  inactive card's scroll range; swiping back now preserves the reading position.
- Temporal survival remains a versioned heuristic, not a calibrated probability.
  This historical milestone preceded indexed retrieval and the source-agreement
  consumer. Those paths now have separate evidence above. Common lexical queries
  and high-degree graphs still have data-dependent costs; semantic policy
  evaluation, engagement consumers and local adaptation remain open.

## Document-Bound Bridge Follow-Up

- Browser Surfaces admit one child-created MessagePort per mount, with bounded
  accept/confirm/ready negotiation and no window-RPC fallback. Responses remain
  on the admitted channel when the iframe's WindowProxy navigates. Frontend
  activation waits for readiness; close, authority changes and lease loss cancel
  startup and cannot be undone by late confirmation.
- Bundled Surface resources use the new handshake. Existing immutable Objects
  are not rewritten; legacy clients need republishing. Initial redirects, remote
  resource integrity and deliberate port delegation remain explicit trust limits.
- Full Rust workspace, 289 frontend tests, 92 SDK tests, schema conformance and
  Astro check/build pass. The real API/Python/Astro/Aegis run checks actual bundled
  Surface RPC success and held-load navigation with security negative controls,
  plus existing social/account/publication/lease/source-agreement workflows.
  `artifacts/live-stack-document-bridge-verified-host.fozzy` passes strict trace
  verification, replay and CI. See [the bridge contract](surface-bridge.md) for
  focused traces, test instrumentation limits and retained failures.
- The current 32px primary, 18px reply and 14px nested hierarchy, tonal backgrounds,
  neighbor visibility, desktop controls and reading-position restoration pass
  the same full integration run. DOM geometry is not screenshot or physical-touch
  approval. This milestone does not close the whole-platform goal.

## Resource Admission And Delivery Follow-Up

- Shared structured URI admission rejects malformed, ambiguous and traversing
  references. Surface integrity, entry and MIME must match the same declared
  resource, regardless of same-hash candidate ordering. Authoring retains its
  duplicate-hash policy. URI admission is not external-byte verification.
- Raw Surface HTTP, media REST and media RPC now share an 8 MiB bounded read,
  canonical blob-hash validation and exact-byte digest verification. Opened-file
  metadata rejects oversized files before buffering; growth checks stop after
  at most one excess byte. Native trusted reads retain their explicit unbounded
  policy. See [the delivery contract](resource-delivery.md) for exact limits.
- Independent review found the JSON extractor's default 2 MiB limit disagreed
  with the existing 16 MiB request ceiling. Both now agree; authenticated REST
  and RPC regressions verify large uploads and rejection before persistence.
- Fourteen focused store/API tests pass in
  `artifacts/resource-integrity-final-native-host.fozzy`, with strict trace
  verification, replay and CI. Thirty-four URI/authoring/runtime tests, including
  2,880 hash mutations and alias permutations, pass in
  `backend/crates/runtime/tests/resource-uri-aliases.trace.fozzy`, also verified,
  replayed and CI-passed. Scripted doctor5/test/property fuzz checks orchestration
  only. Fozzy explore does not accept these sequential process scenarios.
- The first native host trace is retained as failure evidence: deterministic
  clocks exposed test-directory collisions. Fixture roots now also include an
  atomic counter; no product behavior was waived to obtain the passing run.
- The final full Rust workspace test run and changed-file formatting checks pass.
  No protocol DTOs changed; generated SDK contracts and independent Python
  fixture conformance remain current. This pass changes no frontend styling.
- The real Python/API/Astro/Aegis suite passes in
  `artifacts/live-stack-resource-integrity-host.fozzy` (run
  `a32276d7-f8d1-4472-897f-9e4a3ff055ab`), with strict verification, replay and CI.
  It checks live corruption/size rejection and restored exact bytes before
  mounting the Surface, then real document-port RPC and adversarial navigation,
  plus the existing account, social, publication and algorithm workflows.
  Rounded cards, smaller reply hierarchy, horizontal navigation, reading-position
  restoration and geometry at 320/390/860/1440px pass. This is not screenshot
  approval or physical-device gesture evidence.
- At this milestone, [verified executable bundles](verified-bundles.md) remained
  proposed. The local implementation below supersedes that status; remote
  acquisition and deployment-level isolation/resource requirements remain open.

## Rounded Card Polish Follow-Up

- Preserved the deprecated carousel geometry and horizontal transitions, rounded
  32px primary cards, 18px reply rows, and 14px nested replies. Lightened the rose,
  sage, and blue card hues and shadows; nested replies now use smaller insets.
  Neighboring cards remain sharp instead of deliberately blurred. Card and reply
  hover fills are limited to hover-capable pointers.
- All 289 frontend tests pass. Astro check reports zero errors and zero warnings
  with two existing async-conversion hints. Real Aegis DOM checks cover
  320/390/860/1440px layouts, nonoverlapping controls, smaller reply geometry,
  anchored reply refresh, nested Back, and horizontal reading-position recovery.
- `artifacts/card-polish-host.fozzy` records the actual host-backed Aegis checks
  (run `79267143-7b3b-4ab7-97ba-b32bb4be3717`), with strict trace verification,
  replay and CI passing. Strict deterministic doctor (five runs) and test ran
  first; 32 scripted fuzz runs pass as orchestration evidence only.
- This is geometry and behavior evidence, not screenshot approval or a physical
  touch-device check. It does not close the remaining platform readiness gaps.

## Verified Local Bundle Follow-Up

- Implemented immutable verified bundle receipts, native runtime admission,
  opt-in loopback gateway origins, and SDK descriptor validation. Delivery checks
  current account ownership, lease, capability policy and lifecycle; suspension
  retains the snapshot but denies delivery until resumed. Revoked sessions cannot
  regain admission by retrying start.
- `artifacts/bundle-gateway-lifecycle-host.fozzy` records 109 native tests passing
  (97 API, six node bundle, six runtime bundle tests). Strict verification, replay
  and CI pass. The separate API server/source-agreement integration tests pass.
- `artifacts/verified-bundle-browser-final.fozzy` records actual Aegis execution
  through the production gateway and SDK: isolated origins, RPC, immutable bytes,
  corruption rejection, CSP/routes, eviction and account revocation. Wrong-origin
  and wrong-source checks include weakened negative controls. Strict doctor,
  test, trace verification, replay and CI pass. Owned test listeners are closed.
- Full SDK tests pass (102); frontend tests pass (291), including verified mount
  descriptor use and mismatched-descriptor cleanup. Generated contract checks,
  Astro check and production build pass. Production DNS/TLS, remote acquisition,
  compiler integration and hard browser resource enforcement remain incomplete.

## Card Depth Follow-Up

- Kept the rounded 32/18/14px primary/reply/nested hierarchy and tonal rose,
  sage and blue palette. Widened the desktop primary card, softened the side-card
  tilt, increased neighboring card visibility, and adjusted shadows and reply gaps.
- `artifacts/card-style-depth-host.fozzy` records actual Aegis DOM checks at
  320/390/860/1440px, plus conversation anchoring and horizontal navigation.
  Strict doctor (five runs), scenario test, trace verification, replay and CI
  pass. The 32 scripted fuzz runs establish orchestration coverage only.
- No horizontal overflow or overlapping header controls was detected. Screenshot
  approval and physical-device touch verification remain outstanding; geometry
  assertions do not establish visual approval. Preview remains on port 4321.

## Rounded Card Refinement

- Retained the rounded horizontal deck and 32/18/14px card hierarchy, strengthened
  rose/sage/blue tones with matching shadows, and tightened the reply spacing.
  Conversations now follow the primary card directly; technical inspection follows
  the conversation rather than interrupting its connection to the post.
- `artifacts/rounded-card-refinement-host.fozzy` (run
  `027df5ad-1d4b-4c4d-8a10-ba43463df674`) passes 25 focused tests and real Aegis DOM
  checks at 320/390/860/1280/1440px, including hierarchy, overflow, header controls,
  horizontal navigation and reading-position restoration. Strict doctor (five
  runs), scenario test, trace verification, replay and CI pass. The 32 scripted
  fuzz runs are orchestration evidence only. Astro check has zero errors or warnings.
- This is not screenshot approval or physical-device gesture verification. The
  preceding broad bundle-authoring integration run ended with an OS disk-space
  error and is not counted as passing. The recovered run is recorded below.

## Verified Browser Application Authoring

- The composer accepts a built application folder, validates canonical paths,
  MIME mappings, entry selection, file/manifest/transport limits and captured
  byte lengths, uploads authenticated blobs, and publishes the signed inventory.
  Files remain in account-scoped memory across composer close/reopen. Invalid
  picker states cannot silently publish a text-only Object; pending duplicate
  submits are suppressed and retries retain their durable operation identity.
- Exact 8 MiB uploads now fit the bounded HTTP envelope: 16 MiB of hex plus
  64 KiB framing headroom. Decoded blobs remain capped at 8 MiB. Browser hex
  encoding avoids a per-byte JavaScript string array. See
  [the authoring contract](application-authoring.md) for trust and input limits.
- Existing focused traces record 308 frontend tests
  (`artifacts/bundle-authoring-client-final-host.fozzy`) and 111 native tests
  (`artifacts/bundle-authoring-native-host.fozzy`: 99 API, six node bundle,
  six runtime bundle tests). The authoring pass also fixed the composer panel's
  missing fixed positioning; its actual controls fit at 320/390/860px.
- After reclaiming regenerable Rust object files, the complete live suite passes:
  `artifacts/live-stack-bundle-authoring-recovered-host.fozzy`, run
  `0e6c1b5f-fb0d-4cac-b8d7-792f3cb531fe`. Strict deterministic doctor (five runs)
  and scenario test ran first; strict trace verification, replay and CI pass.
  Aegis drives the actual composer, retains Files through close/reopen, rejects
  an invalid directory, publishes once despite duplicate submits, reads back
  signed manifest and exact bytes, and executes HTML/JS/CSS through the isolated
  gateway. Four counter clicks across two fresh mounts work; both sessions are
  evicted on close. Existing account, social, ranking, lease and bridge flows pass.
- The earlier disk failure did not produce a trace. This recovered run, rather
  than any failed attempt, is the current integration evidence. The preview is
  restored at port 4321 against the persistent API and gateway on 8787/8788.
- Source compilation, dependency discovery, capability-request authoring,
  orphan-blob retention/GC, remote acquisition, deployment DNS/TLS, hard browser
  resource enforcement and full platform requirements remain open. This is not
  a declaration of whole-platform production readiness.

## Host Permission Review Verification

- Host-owned permission review now exposes declared scopes and limits, explicit
  approval, revocation of every matching live grant, and reconciled server state.
  Opening review stops the local Surface; opening the Object is a separate action.
  Account changes invalidate the dialog and late responses cannot overwrite it.
- HTTP capability inspection and mutation responses expose only the viewer's
  grants. Capability receipts now use the exact authorized, bound, unexpired
  grant rather than selecting again from all grants for the Object.
- Authenticated upload parsing now shares the execution envelope limit. Exact
  8 MiB decoded files pass REST and RPC; one extra decoded byte is rejected
  without storage, and oversized envelopes fail before ingestion.
- Frontend tests: 320 pass in
  `artifacts/permissions-client-integrated-host.fozzy`. Native API, node and
  capability tests: 293 pass in `artifacts/permissions-api-integrated-host.fozzy`.
  The complete live suite passes in `artifacts/live-stack-permissions-host.fozzy`
  (run `9b8848bb-c2eb-497f-aca7-a62ebedcf69e`), including foreign-authored Object
  approval, actual gateway execution, stop-before-review and persistent revoke.
  Strict deterministic checks ran first; trace verification, replay and CI pass
  using the actual Fozzy engine, not the incompatible PATH installation.
- These traces precede the subsequent card-spacing refinement. At that milestone,
  per-invocation consent was unfinished: `ask_each_time` grants were reusable until
  revoked, and the dialog disclosed that behavior. Browser capability authoring
  was also open. Later scoped milestones supersede those implementation gaps;
  see the current [social invocation status](#social-invocation-milestone).

## Card Spacing And Tonal Refinement

- Preserved the rounded swipe deck, 32px main-card edges, 18px reply edges and
  14px nested-reply edges. Rose, sage and blue remain tied to text, executable
  and image Objects. Main cards have stronger tonal depth on a cool neutral
  background; comments retain lighter hues and smaller dimensions.
- Desktop neighbors now have a visible gap rather than overlapping the active
  card. Reply columns occupy at most 80% of the main card on desktop, with a
  further inset for nested replies. Mobile keeps its compact peeking deck.
- `artifacts/card-style-refinement-host.fozzy` records 25 controller/reading
  tests and real Aegis geometry at 320, 390, 860, 1024, 1100, 1280 and 1440px.
  No horizontal overflow or header overlap; horizontal navigation, nested Back
  and reading-position restoration pass. Strict doctor (five runs), scenario
  test, trace verification, replay and CI pass. The 32 scripted fuzz runs cover
  orchestration only. Astro check reports no errors or warnings (two hints).
- These are DOM/layout measurements, not screenshot-based aesthetic approval
  or physical-device gesture validation; those checks remain outstanding.

## Permission Revocation Propagation

- A successful local revocation immediately retires every native Surface session
  whose admission plan selected that exact grant. Imported grant/revocation
  events reconcile the same authority. Other actors' grants and unrelated
  Objects remain independent; alternate grants cannot rescue an existing
  session. A new explicit launch may use remaining valid consent.
- API execution and periodic cleanup also reconcile expiry and missing grants.
  Retired sessions remain inspectable but cannot execute, renew, reactivate or
  checkpoint. Native tests cover active, prefetched and suspended sessions,
  unauthorized/imported revocations, duplicates, restart and the exact expiry
  boundary, including a backward clock step after retirement.
- 296 tests across API, node and capabilities pass in
  `artifacts/permission-revocation-api-integrated-host.fozzy` (run
  `db0baa4d-2559-40f0-b522-256e4066902c`). The complete live suite passes in
  `artifacts/live-stack-permission-revocation-integrated-host.fozzy` (run
  `32bce73f-199a-4ffb-bd37-17decd5713c8`). Strict deterministic doctor/test ran
  first; both real traces pass strict verification, replay and CI. An additional
  32 scripted fuzz runs cover orchestration, not live browser behavior.
- Aegis uses a separate authenticated RPC request to revoke permission while
  the actual bundle iframe is mounted. It proves immediate server retirement,
  removal by the normal heartbeat, no automatic remount during a further
  16-second observation, blocked retry, and explicit regrant with a new session.
  The original session remains terminal after regrant. Existing account, social,
  authoring, bridge, ranking, card and narrow-layout checks also pass.
- The earlier native failure reflected the old capability-error expectation;
  the earlier browser failure was an unsupported scalar Aegis result in the
  new assertion. Both failed traces are retained; the integrated traces above
  are the current evidence.
- Browser removal is lease-based, not instantaneous push cancellation. Signed
  events must reach other federated nodes before their authority changes.
  Already completed external operations cannot be undone. At that milestone,
  per-invocation consent, browser capability authoring, resource enforcement and
  the whole-platform audit remained open; later sections record scoped progress.

## Durable Consent Idempotency

- Grant and revoke now atomically persist their signed event with a private
  durable receipt. RPC requires its envelope key; REST opts in through one
  nonblank `Idempotency-Key` of at most 256 bytes. Unkeyed REST retains its
  existing behavior. Configured CORS origins may send the header.
- A matching retry returns the original event and the actor's current grant
  projection, including changes since the original request. It does not replay
  a stale response or restore revoked authority. Replaying an old revocation
  also leaves later approvals intact. Method/payload reuse conflicts with REST
  409 or RPC `CONFLICT`; authentication and ownership remain enforced on retries.
  RPC and REST have distinct retry namespaces; see [permissions](permissions.md).
- Store recovery tests cover both event kinds at all 11 journal phases, using
  injected errors and real child-process exits, repeated reopen, malformed
  payloads, orphan/mismatched receipts, corruption, signatures, and compatibility
  with existing Object/edge receipts. Store agent's focused run passed 40 tests;
  `/tmp/babel-consent-store-verified.fozzy` passed strict verification, replay and
  all seven CI checks (run `9bc9cd9a-f879-4509-b100-89f2a69fe7f5`).
- Final integrated host trace:
  `artifacts/consent-idempotency-api-final-host.fozzy`, run
  `be5eb7ed-e4ef-464c-9108-9dfcac87d1ca`, seed 421. All 369 tests pass with no
  failures, ignored tests or compiler warnings. It executes
  `CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p babel-store -p babel-api -p babel-node -p babel-capabilities`.
  Coverage includes concurrent retries, actor isolation, REST/RPC changed-intent
  conflicts, restart, approval retry after revocation, revocation retry after a
  later approval, and CORS preflight for the new header.
- Used the actual `/Users/deepsaint/.cargo/bin/fozzy` engine. Strict deterministic
  `doctor --deep --scenario tests/permissions-api.fozzy.json --runs 5 --seed 421`
  and `test --det --strict tests/permissions-api.fozzy.json` passed before host
  execution. The final `run --det --seed 421` used host process/filesystem/HTTP
  backends and a recorded trace; strict trace verification, replay and all seven
  CI checks passed. Scenario validation, report/artifact inspection and scoped
  Rust formatting checks passed as well.
- The 32 scripted fuzz runs passed, but cover scenario orchestration, not Rust
  input fuzzing. `explore` rejected the process-step scenario because it requires
  a distributed scenario; no distributed exploration is claimed. There was no
  failing feature trace to shrink. Replay checks recorded host observations; it
  does not rerun Cargo or prove host execution is inherently deterministic.
- Browser retry controls were not changed. The updated Aegis live-stack suite
  exercises authenticated HTTP retries directly from the host browser account:
  duplicate grants/revocations return the same event, approval replay reports
  revoked current authority, and an old revocation replay preserves a later
  approval. Current inspection confirms old approval replay cannot admit a
  Surface. Normal UI permission review/revoke/reapproval still passes.
- Final browser trace `artifacts/live-stack-consent-retries-final-host.fozzy`,
  run `ada00fed-5d0b-4989-b311-fd29b2668536`, seed 422, passed in 131 seconds.
  Strict trace verification, replay and all seven CI checks passed. The earlier
  `live-stack-consent-retries-isolated-host.fozzy` run also passed; the two-run
  outcome comparison found no failures, not a statistical reliability claim.
- The live-stack runner now owns an isolated Astro child and temporary cache,
  with strict port binding and shutdown on parent IPC disconnection. It never
  invokes workspace-wide `astro dev stop`. Focused real-process tests cover
  occupied-port rejection without replacing the existing server, actual page
  serving with the isolated API URL, disconnect cleanup and retained service.
  `artifacts/astro-isolation-host.fozzy`, run
  `ce09a4a4-b2e7-4132-a4d3-fdbf7ad32056`, passed strict verification, replay and
  all seven CI checks after five-run deterministic doctor and strict tests.
  Preview 4321 and persistent API/store 8787 remained on their original
  processes; test ports 14329/18787/18788 were unbound after completion. The
  persistent preview API was not restarted onto the new binary in this pass.
- At that milestone, per-invocation `ask_each_time` enforcement, browser capability authoring,
  receipt retention/GC, deployment recovery drills, distributed delivery and
  the single-writer restriction remained open. This is a scoped local durability
  milestone, not a whole-platform production-readiness claim.

## Historical Invocation Foundation Evidence

This section records the pre-integration foundation milestone. The current
[social milestone](#social-invocation-milestone) supersedes its implementation
status, not the scope of its historical traces.

The [invocation consent decision](invocation-consent.md) then had a domain/store
foundation, but no end-to-end authorization integration. Source review found
reusable `ask_each_time` authority and seven sensitive methods that return action
descriptors instead of completed effects. Fresh per-use consent, transactional
quota accounting, authenticated document/session binding and real executors
were still required; unavailable adapters or blanket denial were not completion.

The foundation adds private immutable intent and phase records, exact context
and deadline checks, predecessor-based transition conflicts, unchanged-intent
retry lookup, and `Unknown` reconciliation without redispatch. Local completion
requires its receipt and signed effects in the same existing journal batch;
receipt actor and intent fingerprint must match. Tests cover concurrent decisions
and cancellation versus publication, malformed histories, missing/corrupt effects,
same-actor wrong-intent receipts, unexpected collections, and journal crash/reopen.
Node/API/UI callers had not yet adopted these APIs. Quota debit and lifecycle
invalidation policy were not implemented by that foundation.

The combined API/Python/Astro/Aegis regression passed on that source, including
native MessageChannel cancellation and existing social/publishing/Surface flows:
`artifacts/live-stack-invocation-foundation-host.fozzy`, run
`4335d1bf-d523-4de0-8406-955a810b841d`, seed 424, 147 seconds. Strict trace
verification, replay and all seven CI checks pass. The isolated test services
did not replace preview 4321 or persistent API 8787; their original PIDs remain.
This live suite checks regression compatibility, not the not-yet-wired invocation
workflow. Source review and focused domain/store tests are separate evidence.

Final integrated native tests for `babel-store`, `babel-api`, `babel-node`, and
`babel-capabilities` also pass, including the final fractional-deadline regression:
`artifacts/invocation-foundation-integrated-host.fozzy`, run
`cac3a567-c4e1-4dbe-8a60-5bb4f56dd32c`, seed 424. Strict deterministic doctor
(five runs) and test preceded host execution; strict verification, replay and all
seven CI checks pass. No running compiler or test process is needed by this
milestone. The persistent preview API was not restarted onto this binary.

## Cooperative Bridge Cancellation

- Browser transport abort, timeout and close now send an exact per-request
  cancellation message. The host checks the original dispatch origin, aborts
  cooperative work, suppresses late output and retains capacity until settlement.
  Each attempt gets a fresh wire ID; caller IDs are restored on response. Late
  replies from older hosts cannot resolve a later attempt with a reused caller ID.
- SDK tests cover independent requests, noncooperative capacity, timeout/close,
  malformed and cross-origin controls, pre-cancellation, repeated cancellation,
  reentrant timers, stale legacy replies and failed structured-clone sends. The
  response-origin test uses the actual wire ID and a trusted-response control.
- Real Aegis/native MessageChannel verification starts a real HTTP fetch and
  confirms caller abort closes that HTTP response, then reuses the caller ID
  while injecting the old response before the new result. The existing document
  navigation test and its two deliberately unsafe controls also pass.
- Final SDK host trace `artifacts/bridge-cancellation-sdk-final-host.fozzy`, run
  `da7d7e87-d5f5-4a8d-ba71-0bd65f7139b9`; frontend host trace
  `artifacts/bridge-cancellation-frontend-host.fozzy`, run
  `96ec7708-eb0c-4e95-8666-e28d2ebf3a7a`; standalone Aegis host trace
  `artifacts/bridge-cancellation-aegis-host.fozzy`, run
  `3261756f-1b1f-42c9-b7cb-41356ed64286`. All use seed 424 and pass strict trace
  verification, replay and all seven CI checks after deterministic doctor/test.
  Astro check has zero errors/warnings and two existing hints; production build
  passes. No screenshot or physical-touch verification is claimed.
- The initial SDK trace preserves a send-failure regression, now fixed; its
  minimized failure trace is `artifacts/bridge-cancellation-sdk-failure-min.fozzy`.
  Thirty-two scripted fuzz runs pass but test orchestration, not arbitrary SDK
  inputs. Distributed exploration does not support these process-step scenarios.
- This is not durable invocation cancellation, backend transaction rollback or
  completed per-use consent. Admitted server work may still commit after HTTP
  disconnect. See [Surface bridge](surface-bridge.md#per-request-cancellation).

## Composer And Image Viewing

- The composer now has a rounded, focused post/reply/share layout, current author,
  attachment icon controls, image preview/removal and retained built-app entry
  selection. The existing draft and publishing controllers still own data and
  retries. Native picker cancellation retains the attachment; replacement/removal
  releases obsolete preview URLs; account changes keep drafts isolated.
- Image posts open in a full-image dialog with fit, bounded zoom, mouse panning,
  native scrolling, keyboard controls, loading/error/retry and focus restoration.
  Image taps open the viewer; horizontal image drags still navigate the deck
  without opening it. The underlying card and reading position stay intact.
- All 338 frontend tests, Astro check and production build pass. The check has
  zero errors/warnings and two existing async-conversion hints. Host trace
  `artifacts/social-media-ui-host.fozzy`, run
  `156f30c1-cda4-429f-80b1-ac64c68c1dfc`, seed 425, passes strict verification,
  replay and all seven CI checks after five-run deterministic doctor and strict
  test. Thirty-two scripted fuzz runs pass for orchestration only. Distributed
  exploration does not support these process-step scenarios.
- Real Aegis checks cover preview/removal/reattachment followed by publication,
  opening the published image, zoom/reset, blocked deck navigation while viewing,
  restored focus/reading and 1280/390/320-pixel same-origin frame layouts. These
  are runtime and geometry checks, not screenshot taste approval or physical
  touch testing. Screenshot-based aesthetic and physical-touch review remain open.
- Earlier live runs caught outdated test assumptions about the old Author label
  and text-only attachment controls. They are retained as failure evidence. The
  updated checks assert the current author value/read-only/accessibility label,
  visible text bounds, attachment labels/icons and 44-pixel hit targets. Clipped
  screen-reader labels and icons over their native file inputs are not treated
  as accidental visual collisions.
- Final complete live-stack trace:
  `artifacts/live-stack-social-media-ui-final-host.fozzy`, run
  `df7d9c25-9e5c-4cfd-9f1e-7c54120ba9fb`, seed 425, passed in 139 seconds.
  Strict trace verification, replay and all seven CI checks pass. Existing
  social, application-authoring, permissions, leases and account workflows also
  pass. Preview 4321 and API 8787 retain their original processes; isolated test
  ports are closed. This completes the composer/image-viewing slice, not all
  social functionality or interactive Object executors.

## Clipboard And Fullscreen Host Actions

Historical local-confirmation milestone. The later [durable browser actions](#durable-browser-actions)
section supersedes its v1 grant and invocation limitations.

- Astro now executes server-authorized clipboard writes and fullscreen requests
  after a trusted per-use modal confirmation. Clipboard previews use literal
  text; fullscreen targets the host Object panel, never a child-provided selector.
  Native completion determines bridge success. Unsupported APIs, native denial,
  failed effects, cancellation and expired requests return errors.
- A mounted Object admits one outstanding browser action, including authorization
  and native work. Account/lease changes, visibility loss, bridge cancellation
  and close remove prompts. Closing exits only fullscreen owned by that host.
  Already-admitted clipboard writes cannot be undone. This is local confirmation,
  not completion of durable invocation consent or the remaining external executors.
- All 357 frontend tests, Astro check and production build pass. The check has
  zero errors/warnings and two existing hints. The host trace
  `artifacts/host-actions-host.fozzy`, run
  `f183cadd-a8ed-45b1-9ed7-25950ecc8efb`, seed 426, passes strict verification,
  replay and all seven CI checks after five-run deterministic doctor and strict
  test. Thirty-two scripted fuzz runs check orchestration only; distributed
  exploration does not support these steps scenarios.
- Initial live traces caught invalid fixture scope (`null` instead of an Object)
  and a missing required RPC idempotency key. The fixture now supplies both and
  fails immediately when an RPC response arrives instead of the expected prompt.
  Those failed traces remain as evidence, not successful product verification.
- The full API/Astro/Aegis suite passed in 154 seconds with trace
  `artifacts/live-stack-host-actions-verified-host.fozzy`, run
  `113624a3-9458-429a-a51e-583da31cc161`, seed 426. Strict verification, replay
  and all seven CI checks pass. The new test publishes a signed executable bundle,
  grants the viewer access through the UI, and requests actions through the real
  private iframe bridge. It verifies literal clipboard preview, denial, default
  cancel focus, bounded concurrency, prompt/control geometry and close cleanup.
  Fullscreen's programmatic confirmation produced the expected native failure,
  not false success. Successful native fullscreen and clipboard effects are
  unit-tested with controlled APIs, not proven with physical browser input.
  The suite deliberately does not overwrite the machine's real clipboard.
- An early stylesheet import temporarily broke the live preview before the
  delegated file existed. Once files were present, invalidating the Astro entry
  cleared the cached missing-module error. HTTP checks returned 200 for the page,
  main module and stylesheet; Aegis loaded the feed Online. The original preview
  process and user account data were preserved. Future integrations must create
  dependencies before adding imports to a live entrypoint.

## Browser Capability Authoring

- The app composer now exposes clipboard/fullscreen checkboxes and an advanced
  protocol capability-array editor. Common controls preserve other scoped or
  custom declarations. Permissions are captured in the signed Object; selecting
  them does not grant authority or supply missing external executors.
- Raw declaration text, including malformed intermediate edits, stays with the
  account-scoped in-memory attachment. It survives close/reopen, stays hidden
  across account switches, is locked during submission, and never enters local
  storage. Invalid structural declarations block publication before upload.
  Semantic scope validation remains authoritative on the backend. See
  [the input contract](application-authoring.md#declared-permissions).
- All 372 frontend tests, Astro check and production build pass. The check has
  zero errors/warnings and two existing hints. Host trace
  `artifacts/bundle-permissions-host.fozzy`, run
  `764643b7-f942-4f40-bce3-07a88b4f5a54`, seed 427, passes strict verification,
  replay and all seven CI checks. Five-run deterministic doctor and strict test
  ran first. Thirty-two scripted fuzz runs cover orchestration only.
- The real API/Astro/Aegis suite passes with trace
  `artifacts/live-stack-bundle-permissions-verified-host.fozzy`, run
  `95f5a4c4-3e5a-4fc3-a3bc-6405e3efa446`, seed 427, in 154 seconds. Strict
  verification, replay and all seven CI checks pass. It retains an invalid
  declaration draft through close/reopen, proves it cannot publish, edits scoped
  storage plus common permissions, reads back the signed declarations, approves
  access through the UI, mounts the app and performs a real storage write/read.
  Clipboard preview/denial, request concurrency and close cleanup pass through
  the same app. Native clipboard writing is not exercised; programmatic
  fullscreen confirmation returns the expected activation error rather than
  false success.
- Expanded declaration controls pass actual-page geometry checks at 320, 390
  and 860 pixels. Initial live runs caught a wrong search-response assumption
  in the new helper and a dialog hit-target measurement taken during transition.
  The helper now uses `results[].object`; geometry awaits active animations and
  permits only 0.01 pixel of rounding. Failed traces remain as evidence. These
  checks are not screenshot taste approval or physical-touch validation.
- Earlier milestone notes listing browser capability authoring as open are
  superseded by this section. Durable per-invocation consent was still open at
  this milestone; see the newer [social integration](#social-invocation-milestone).
  Missing external executors, source compilation and complete rich-object
  creation remain open.

## Rich Media Follow-Up (2026-09-30)

- [Rich media](rich-media.md) now runs through the real composer, signed media
  publication, Object-bound binary delivery, and native audio/video players.
  The approved rounded swipe cards, vertical conversations, image viewer, and
  existing actions remain in place. Audio/video previews retain draft ownership
  and unload obsolete resources. Feed players pause when inactive, hidden,
  offscreen, or removed; cached conversation players can be reattached.
- Binary `GET`/`HEAD /objects/{id}/media/{hash}` derives an inert MIME type from
  the signed resource, checks its exact local URI and full integrity, and enforces
  the existing 8 MiB bound. Single byte ranges, suffix/open ranges, ETag/If-Range,
  correct HEAD bodies, and rejection paths have real Router/HTTP tests. This is
  bounded buffered delivery, not large-file streaming. Final focused backend
  trace `backend/crates/api/tests/media-verified-host.trace.fozzy` passes strict
  verification, replay and all seven CI checks.
- All **403 frontend tests**, Astro check, and production build pass. Check has
  zero errors/warnings and two existing hints. Final host trace
  `artifacts/rich-media-frontend-final-host.fozzy`, run
  `e3d59c42-1183-4cc3-9f60-a63437bb25b2`, seed 429, passes strict verification,
  replay and all seven CI checks. Strict deterministic doctor/test ran first;
  32 scripted fuzz runs cover orchestration, not real media decoding.
- The complete isolated API/Astro/Aegis suite passes in 169 seconds. Trace
  `artifacts/live-stack-rich-media-layout-host.fozzy`, run
  `a6f9386c-d94c-4acd-826f-ec3c6669d82b`, seed 429, passes strict verification,
  replay and all seven CI checks. It publishes real WAV and WebM files through
  the composer, verifies preview retention, decoding, range bytes and seeking,
  advances actual muted video playback, and checks swipe pausing/no automatic
  resumption. Audio/video layouts fit at 320, 390 and 1280 pixels. Existing
  image, profile, reaction, following, permission, host-action, account-security,
  and embedded-app regressions also pass.
- Aegis synthetic events do not grant trusted audio-play activation, and its
  installed Chromium build does not decode H.264. Audio's actual user-initiated
  playback and H.264 decoding therefore remain manual/browser-specific checks,
  not claimed automated successes. Native audio decoding/seek/control checks do
  pass; unexpected errors are not ignored. Layout measurements are not screenshot
  approval or physical-touch validation. No autoplay-policy bypass was added.
- Earlier failed traces are retained. They exposed the renamed picker label,
  automation activation/codec limits, a quoting error in the new layout helper,
  and an existing session-restore harness issue: Aegis may no-op same-URL
  navigation. The restore check now uses a distinct URL, a fresh-document guard,
  and the settled feed before interacting.
- Preview API 8787 was restarted onto the new binary with its existing store and
  configuration. The `deepsaint` identity remains present. Astro preview 4321
  stayed on its original process; test ports 14329/18787/18788 were released.
  Test publications remained in disposable stores.
- Images remain limited to 4 MiB; audio/video to 8 MiB. Albums, caption-track
  authoring, large/resumable uploads, transcoding, capture, and media attachments
  in the reply/share composer are not implemented by this pass. The broader
  requirement audit and active goal remain open.

## Media Conversations Follow-Up (2026-09-30)

- The reply/share composer now uploads images, audio, or video with an optional
  caption. It keeps the existing rounded UI, per-target drafts, native previews,
  pending controls, account ownership, and durable retry keys. Application
  bundles remain publish-only. Replies update the current conversation; shares
  select their new media card. This supersedes the previous section's missing
  media-attachment composer limitation, not its other open work.
- Optional `media: { title, resources }` uses the canonical generated RPC schema.
  The node checks exact blob URI, normalized MIME, presence, actual size, and full
  integrity before publishing the signed Object and relationship together. The
  existing journal covers the publication and retry receipt. Social request quota
  counts caption/reference bytes, not uploaded bytes again. Generic MIME is
  permitted as inert protocol data; executable MIME does not acquire a Surface
  and cannot use the inline binary delivery route.
- Final backend host trace
  `backend/crates/api/tests/social-media-final-host.trace.fozzy`, run
  `763027db-120e-4259-b453-8fbb27fbe0e9`, seed 431, passes all nine focused tests,
  strict verification, replay and all seven CI checks. Coverage includes real
  authenticated Router requests, stolen author/controller rejection, grant
  revocation, retry after restart, and invalid-resource/atomic-failure paths.
  Broader backend runs and targeted completion runs passed 205 API, 110 node,
  and seven schema tests. One parallel `session_permissions` fixture collision
  required a serial rerun; that existing test-isolation issue is not resolved.
- All **441 frontend tests**, Astro check, and production build pass in
  `artifacts/social-media-composer-host.fozzy`, run
  `44d089c9-badf-4116-af8a-5e06c6474bb6`, seed 431. Strict trace verification,
  replay, and all seven CI checks pass. Check has zero errors/warnings and two
  existing hints. The 38 new tests cover captionless media, full captions,
  target/mode isolation, upload/publication failure retries, delayed file reads,
  session invalidation, and preservation of newer edits. Strict deterministic
  doctor/test ran first; 32 scripted fuzz runs test orchestration only.
- Preview API 8787 was restarted with the existing store and configuration before
  social attachment controls were enabled. The existing identity remains
  accessible. No account session was reset and no test content was added to the
  user's persistent store.
- The complete API/Astro/Aegis suite passes in 187 seconds in
  `artifacts/live-stack-social-media-retest-host.fozzy`, run
  `9a92ccee-79e0-4df9-a99e-12850444123c`, seed 431. Strict verification, replay,
  and all seven CI checks pass. It posts real PNG/WAV/WebM attachments through
  both reply and share controls, verifies their rendered media, reads back each
  persisted Object and exact target edge, and compares the delivered bytes. The
  existing draft, image, playback, layout, profile, reaction, following, embedded
  app, permission, host-action, and account-security checks also pass.
- The first full trace, `artifacts/live-stack-social-media-host.fozzy`, passed
  every new media case but timed out waiting for a mobile account iframe in the
  later security test. The test now records the failed readiness stage and
  non-secret DOM state. The rerun passed without changing account production
  logic or extending the timeout; the intermittent readiness failure is retained
  as unresolved evidence, not claimed fixed. Aegis audio activation, H.264,
  screenshot approval, and physical-device limitations from the prior section
  still apply. Full platform completion remains unproven.

## Shared-Post Navigation (September 30, 2026)

- Added the public quote-context HTTP/RPC projection and generated SDK contracts.
  Only signed Quotes edges belonging to the source Object's author are shown;
  deterministic target deduplication, source-bound cursors, historical signing
  keys, import ordering, restart, and unavailable targets are covered.
- Compact rounded previews sit between the primary card and its conversation.
  Opening a quoted original uses the full renderer without automatically
  mounting a Surface or playing media. Bounded visit history restores the feed,
  reading anchor, and focus; profile detours suspend and restore it. Account
  changes discard the old navigation and quote cache. Integration tests exposed
  and fixed Following controls becoming visible on return to a quoted post.
- All **517 frontend tests**, Astro check, and build pass in
  `artifacts/quotes-session-ui-host.fozzy`, run
  `be8d68f0-1122-4cae-ba9e-87b684a11e80`, seed 432. Strict trace verification,
  replay, and all seven CI checks pass. Check reports zero errors/warnings and
  two existing hints. New coverage includes 32 quote-panel, 19 actual RPC
  adapter/transport, six visit-controller, 16 main-navigation tests, and one
  evicted-panel/reused-card regression.
  Strict deterministic doctor/test ran first; 32 scripted fuzz iterations cover
  orchestration only, not browser behavior or protocol input fuzzing.
- Backend verification passes 336 broader tests and eight focused quote tests.
  The final focused host trace is
  `backend/crates/api/tests/quotes-final-host.trace.fozzy`, run
  `a73f638c-7654-462f-938d-a60404c5beb3`, seed 432; strict verification, replay,
  and all seven CI checks pass. SDK generation and TypeScript checks pass.
  Quote cursors are live ordered bookmarks, not frozen snapshots.
- The first two full-browser traces reproduced a real 68px return-position
  shift for audio shares (`artifacts/live-stack-quotes-host.fozzy` and
  `artifacts/live-stack-quotes-reading-diagnostic.fozzy`). The primary audio
  card now reserves its feedback row while hidden, so metadata readiness does
  not collapse the layout during restoration. Browser regression assertions
  check equal loading/ready heights rather than loosening the scroll assertion.
- `artifacts/live-stack-quotes-final-host.fozzy` passed all new media/share
  navigation checks, including three levels of originals, then reproduced the
  mobile session-list readiness failure. A focused failing test reproduced a
  session metadata restoration invalidating an in-flight private request despite
  unchanged credentials. Account revisions now advance on credential boundaries,
  not same-session metadata updates. Two regression tests verify preservation of
  those reads and rejection after logout/login, even with a reused token. No
  timeout was increased and no authentication check was removed.
- The complete API/Astro/Aegis rerun passes in 175 seconds in
  `artifacts/live-stack-quotes-session-host.fozzy`, run
  `d96ed5a8-a506-4d60-aa7c-00ecd6cf42d5`, seed 432. Strict verification, replay,
  and all seven CI checks pass. This covers PNG/WAV/WebM replies and shares,
  six original-post round trips through nested shares with exact scroll/focus
  restoration, loading/ready media layout stability at 1280/390/320 widths,
  and the existing full-stack workflows. Mobile account settings pass at both
  390 and 320 widths, alongside password change and individual/other/current
  session revocation. The earlier failed traces remain as regression evidence.
- This is not full platform completion or physical-device/screenshot approval.
  Deep navigation after quote-cache eviction is not covered by the live suite;
  bounded history, component eviction, and card reattachment have unit coverage.
- The preview's reported `host-actions.css` SSR import failure is no longer
  present: Aegis confirms the live feed root and loaded stylesheet at port 4321.
  The API was restarted against the same persistent store; the existing identity
  remains accessible. No local account data was reset.

## Device-Local Feed Controls

- The Astro Settings panel now exposes the existing local model through interests,
  expertise, word filters, hidden authors, creator affinity, and all four ranking
  overrides. Apply retains the active tab; account changes reset the form. The
  panel has roving keyboard tabs, explicit confirmations, storage errors, focus
  return, and an unframed expandable protocol-details section.
- Object actions include author hiding with a real, account-bound Undo. Saved
  filters remove matching loaded cards immediately and invalidate suspended
  profile/Object/Surface navigation before reloading the feed. Direct profile,
  quoted-original, comment, and acknowledged-publication access remains explicit.
- Ranked feeds and chronological Following share the SDK's Unicode whole-token
  filter and full public text/title/description/alt projection. Following is not
  reranked by affinity or interest settings. Filtered-empty pages offer recovery.
- Validation retains malformed stored data with a warning, rejects invalid or
  oversized models before writes, and bounds each term field to 128 distinct
  words so the SDK cannot silently discard accepted extra words. Reset preserves
  history; clearing history preserves preferences, drafts, and the session.
- The data remains partitioned by API origin and account, including guest state.
  Prior cross-tab changes reject stale saves; history events do not trigger feed
  refresh loops. This is not transactional cross-tab storage or encrypted sync.
- Verification: 578 frontend tests and 116 SDK tests pass, with Astro check/build
  reporting zero errors/warnings and two pre-existing hints. Host trace
  `artifacts/feed-preferences-verified-host.fozzy`, run
  `1f1617aa-8adc-4caa-988b-b33483d62543`, seed 435, passes strict trace verification,
  replay, and all seven CI checks. Tests include real production-main integration,
  validation and quota failures, account/stale-edit boundaries, preserved drafts,
  Undo, Unicode/full-caption parity, and exclusion of every private model field
  from discovery RPC payloads.
- Strict five-run scenario doctor, strict scenario tests, and a 25-run scripted
  fuzz pass are orchestration evidence only; they are separate from the real
  SDK/frontend process results above.
- The focused real API/Astro/Aegis journey passes in
  `artifacts/preferences-focused-browser-host.fozzy`, run
  `46c12d56-6fc5-4513-9dc2-57faaa2957ba`, seed 435, including strict verification,
  replay and all CI checks. It covers immediate author hiding, Undo, filtering
  across all four lenses, empty-state recovery, explicit zero/ranking/affinity
  readback, history confirmation and preservation, reload, guest/account
  separation, signed-in restoration, reset, and panel/control geometry at
  1280/390/320 px. The geometry check is real DOM evidence, not screenshot or
  physical-touch approval.
- `BABEL_LIVE_FOCUS=preferences` selects this bounded journey using the same real
  isolated stack and seed helpers as the full suite; absent that variable, the
  complete suite still runs. Fozzy entrypoints are
  `tests/feed-preferences-browser.fozzy.json` (scripted orchestration) and its
  `-host` counterpart (actual execution).
- The complete live suite also passes in
  `artifacts/live-stack-feed-controls-final.fozzy`, run
  `090faaee-d1db-411f-809a-b011ca6a8016`, seed 435 (195 seconds), with strict trace
  verification, replay, and all CI checks. It includes the new preferences
  journey followed by 21-post Following pagination and account security; existing
  publication, draft, image/playback, reply/share, quote navigation, profile,
  reaction, signed app, verified resource, permission, host-action, and responsive
  card checks remain green. This does not close the broader audit below.
- Earlier failed traces remain for diagnosis: the first browser check incorrectly
  assumed filtering the word `post` must also remove an allowed related candidate;
  subsequent reload harness attempts exposed an Aegis renderer detach, omitted
  primitive eval return values, and same-URL navigation no-ops. The verified
  check uses an object-valued location read and Aegis navigation with a fresh URL
  marker. No application filter was weakened to satisfy the incorrect assertion.
- These controls are device-local filtering, not protocol blocks, reports,
  private graph mutes, or signed moderation. Those broader workflows remain in
  the audit below. See [feed-preferences.md](feed-preferences.md) for the contract.

## Ordered Media Albums

- Posts, replies, and shares now accept ordered image/audio/video albums. The
  composer appends selections, previews individual files, reorders/removes them,
  and preserves immutable submission snapshots through failures and account
  changes. Client limits are 12 files and 64 MiB total, with the existing 4/8 MiB
  per-file limits. These are not protocol resource-count limits.
- Main cards, replies, and expanded parent context expose every supported album
  attachment. Explicit controls and scoped keys navigate the album independently
  of post swipes. Native playback is opt-in; switching attachments pauses and
  releases the outgoing player. Single-file posts keep their existing renderer.
- Canonical media payload order and membership govern album display, even when
  the outer resource list is shuffled or also contains Surface assets. Descriptor
  mismatches and ambiguous outer hashes cannot select unintended media. Custom
  Object payloads retain their own resource semantics.
- Backend publication now validates canonical metadata, exact primary/resource
  agreement, delivery descriptors, blob integrity, and actual byte counts before
  committing a post and its associated edges. Generic signed-record publication
  follows the same checks. Durable retries and recovery retain their existing
  receipt semantics.
- Backend verification passes 189 regression tests, including 10 new album
  cases. `artifacts/media-album-backend.fozzy`, run
  `be1790d8-009a-460c-924b-565b3cf3ccfa`, and
  `artifacts/media-album-backend-regressions-host.fozzy`, run
  `92d9925f-514a-4a27-9226-7ed357d8e90a`, cover 13-resource albums, authenticated
  publish/reply/share, exact byte delivery, order, explicit primary, corrupt and
  missing blobs, metadata rejection, restart, retries, and atomic store recovery.
  Host traces pass strict verification, replay, and CI.
- `artifacts/media-albums-ready-client-host.fozzy`, run
  `7c891246-6914-4b7d-8f55-81a9f4e9aaa3`, seed 436, passes 617 frontend tests,
  116 SDK tests, Astro check, and production build. It passes strict verification,
  replay, and all seven CI checks. Astro reports two existing hints, no errors
  or warnings.
- The focused real API/Astro/Aegis trace
  `artifacts/media-albums-browser-controls-host.fozzy`, run
  `d2c745cd-bec3-4a0e-97be-934ad6a1c38e`, seed 436, passes strict verification,
  replay, and all CI checks. It covers append/reorder/remove, duplicate-content
  recovery, post/reply/share publication, exact signed order and delivered bytes,
  scoped keyboard navigation, selected-image viewing, real muted video playback,
  decoder release, and full/compact galleries plus a 12-file composer at
  1280/390/320 px. Long-title player controls retain at least 44 px height.
  This is real DOM geometry, not screenshot or physical-touch approval.
- The complete live stack passes in
  `artifacts/live-stack-media-albums-verified-host.fozzy`, run
  `c2be2aea-c572-4841-b12f-d41acc6e89ee`, seed 436 (227 seconds), with strict
  verification, replay, and all seven CI checks. It includes the new album
  journey and scripted image-to-post swipe, then existing profile/reaction,
  embedded-app, permission/revocation, host-action, feed-preference, Following,
  and account-security workflows. The isolated test stack and store are cleaned
  up after the run; the normal preview remains available at 4321.
- Earlier traces retain actual findings: an intermediate unit run encountered
  unmigrated module stubs; the first browser run had a polling predicate bug;
  a full-suite run was interrupted by source hot reload; and the expanded
  long-title check caught compressed mobile audio controls. The final layout
  allocates dedicated playback/feedback rows and an independently scrollable
  title region; assertions were not weakened to accommodate clipping.
- A later full run passed albums and permissions but exposed a Settings-test
  precision issue: 44 px checkbox labels reported 43.999969 px bounds during
  translation. The test now waits for the panel animation and requires both a
  44 px layout height and the existing 0.1 px bounds tolerance used by other
  controls. The application layout did not need changing.
- The API at 8787 was restarted with the new binary against its existing
  `.babel-node` store. The existing identity's readback was identical before and
  after restart. No account or store reset was performed.
- Five-run strict scenario doctors and scripted scenario tests pass. A 32-run
  scripted fuzz pass covers orchestration only. Distributed exploration does not
  apply to these step scenarios; host-backed backend fuzz was rejected by the
  installed Fozzy runtime, so no backend fuzz coverage is claimed.
- Caption tracks, large/resumable uploads, transcoding, capture, and the broader
  platform audit remain open. Albums do not imply those features are complete.

## Server Document Binding (September 30, 2026)

Executable Surfaces now register one immutable document with the authenticated
node after port confirmation and before readiness or RPC dispatch. The record
is scoped to the originating login and Surface session. The trusted host injects
the document header; the API rechecks it under the execution lock. Exact retries
are accepted, replacement documents conflict, and closed, revoked, expired or
restarted document-bound sessions cannot regain execution through registration.
Static mounts and unbound management RPCs do not acquire document authority.

- The full API package passes 221 tests in
  `backend/crates/api/.fozzy/documents-host.fozzy`, run
  `8b96f835-43a0-4e31-a04c-d41ee5e12152`, seed 930. Coverage includes persistence,
  migration, concurrent registration, login isolation, malformed/duplicate/stray
  headers, real storage writes, revocation, expiry, and restart behavior.
- `artifacts/surface-document-client-verified-host.fozzy`, run
  `3355a8f1-4590-41c7-81e4-02fd38924f0c`, seed 438, passes 145 SDK and 638
  frontend tests, Astro check, and production build. Check reports zero errors
  and warnings with two existing hints. Native MessageChannel tests cover the
  asynchronous registration gate, cancellation, deadlines, and late settlement.
- `artifacts/surface-document-browser-host.fozzy`, run
  `50d5bf67-ee88-454d-88c3-a4303083d331`, seed 438, verifies the real signed,
  content-addressed seed HTTP Surface. It is not the verified-bundle gateway.
  It proves registration precedes readiness, successful authenticated RPC uses
  the trusted header, identical retry succeeds, a replacement conflicts, another
  login cannot claim the document, and unregistered/mismatched/evicted RPC fails.
- The complete live stack passes in
  `artifacts/live-stack-surface-document-host.fozzy`, run
  `3fcde827-8be5-4a56-b2a8-5cc53b2e2791`, seed 438 (223 seconds). Existing media
  albums, social flows, publishing, account security, permissions, host actions,
  leases, card navigation, and responsive DOM geometry remain covered. This is
  not screenshot approval or a physical-touch/native-activation certification.
- `artifacts/verified-bundle-document-current-host.fozzy`, run
  `765838fa-eb49-4b09-89af-6cd80779da39`, seed 438, separately passes real signed
  bundle execution through the snapshot gateway, authenticated RPC, immutable
  bytes after disk corruption, CSP restrictions, source/origin negative controls,
  eviction, and account-session revocation. Each admitted negative-control
  document owns a separate session. Correlation uses protocol trace metadata,
  preserving the SDK's fresh per-attempt transport IDs.
- These host traces pass strict trace verification, replay and CI. Five-run
  strict scenario doctors and strict scripted tests passed before host runs;
  the 32-run scripted fuzz check covers orchestration, not real browser fuzzing.
  Earlier failed traces are retained: one exposed a missing `performance` global
  in the frontend VM fixture; bundle tests needed the required registration hook
  and correlation updates for generated transport IDs. Production assertions
  were not bypassed to make the harness pass.
- The main API was restarted against the existing store; identity readback was
  unchanged. Aegis confirms the preview at 4321 is online with nine loaded
  Objects, no error overlay, and `host-actions.css` loaded (3101 bytes). No user
  account reset or styling redesign was performed for this milestone.

This document-binding milestone did not verify per-action consent. Durable
approvals, one-time consumption and quota accounting are now implemented for
four social methods in the [new social milestone](#social-invocation-milestone),
whose focused browser acceptance now passes. The seven remaining external
methods and the broader platform goal remain unfinished; see
[invocation consent](invocation-consent.md).

## Audit Still Required

The conversation follow-up now uses reply-ID reading anchors, recognizable nested
parent context, heading focus, and a native scrollbar. Its focused real-DOM Aegis
trace is `artifacts/conversation-reading-host.trace.fozzy` (verified/replayed/CI).
See `design-research.md` for its exact scope and remaining visual/device checks.

| Area | Source Of Requirements | Required Proof |
| --- | --- | --- |
| Identity, objects, graph, provenance | `spec.md` sections 3-7 | Canonical/signature invariants, key transitions, import ordering, tamper and ownership tests. |
| Algorithms and Judgment | Deprecated algorithms, active Python modules, spec sections 9-13 | Per-algorithm migration matrix, full behavior tests, type checks, and evidence that intended runtime discovery uses the implementations. |
| Sandbox and capabilities | Spec sections 6, 14-15; SDK contract | Adversarial untrusted-Surface tests, actual resource enforcement, permission lifecycle, host-action completion and denial behavior. |
| State, storage, federation, finality | Spec sections 8, 16-18 | Durable restart/recovery, real peer operation, partitions/replay tests, and bounded workloads. |
| Moderation and privacy | Spec sections 19, 21 and privacy addenda | Abuse/report/block/mute workflows, local-data boundaries, explanation and error-state evidence. |
| Authoring and rich objects | Spec section 20; user instructions | Real creation, validation, publication, rendering, interaction, and readback for supported content. |
| Performance and operations | Spec sections 22-24 | Representative loads, defined budgets, bounded resources, observability, recovery, deployment configuration and regression gates. |

Spec sections are duplicated in later addenda. Reconcile both sets during the
requirement audit; do not silently discard the later text.

## Verification Rules

- Use the actual Fozzy determinism engine at `/Users/deepsaint/.cargo/bin/fozzy`
  on this machine. The earlier PATH entry currently resolves to a different CLI.
- Scripted `proc_when` scenarios check deterministic orchestration, not whether
  the real child process passes. Host-backed runs are separate evidence.
- `tests/live-stack-host.fozzy.json` runs the actual Rust API, Astro, and Aegis
  suite without assuming the test has empty stdout/stderr.
- Record strict trace verification, replay and CI results with the revision of
  the behavior under test. Do not reuse an old green trace for changed behavior.
- Keep the goal active until the complete requirement audit is satisfied.
