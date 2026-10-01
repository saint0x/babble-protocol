# Private Social Safety

Implementation contract, September 30, 2026. Node, API, host UI and focused real
browser acceptance are verified below. The wider platform goal remains open.

## Behavior

Account-level mute and block are private signed relationship state, not public
Objects, public graph edges, discovery metadata or Judgment inputs. They persist
on the user's node across login sessions. Existing device-local hidden-author
preferences remain separate and keep their existing meaning.

- Mute excludes the author's posts from discovery and Following in the host.
  Explicit profile/Object navigation remains available. Mute does not prohibit
  interaction or notify the target.
- Block also hides that author's posts and prevents new local-node interactions
  in either direction: follows, replies, shares, reactions and explicit graph
  relationships to the other author's Objects. Removing a follow or reaction
  remains possible. Existing signed history is preserved. A block does not make
  public content private or control other nodes, logged-out readers or alternate
  identities. Existing follow state is retained but its posts are excluded while
  the block is active. The host also omits blocked authors from reply rows and
  quoted-post previews; muting alone does not hide explicitly opened conversations.
- Block and mute are independently reversible. Unblocking must not silently
  unmute. Each action requires explicit user choice, displays errors and restores
  actual confirmed state after conflicts. Transport retries retain their exact
  request key; different intent cannot reuse a receipt.

## Host-Only API

All routes require the authenticated account. Embedded Objects never receive
these routes, credentials or relationship snapshots. Responses are `no-store`.

- `GET /social/safety`: `SafetySnapshot` for the current account.
- `GET /social/safety/{identity}`: current `SafetyState` for that pair.
- `PUT /social/safety/{identity}`: `{blocked, muted, expected_revision,
  idempotency_key}`; returns the accepted `SafetyState`.

`SafetyState` is `{author_id, target_id, blocked, muted, revision}`. An untouched
pair has revision zero. Mutations use compare-and-swap and durable exact-request
receipts. Same-intent replay returns its original outcome, not permission to
reapply an old change. Clients refresh the current state after every mutation.

`SafetySnapshot` is `{author_id, revision, entries}`. Each entry is
`{identity, state}`, where `identity` is the existing public Identity record and
`state` belongs to the authenticated owner. Entries contain only active blocks
or mutes, sorted by target identity. The store bounds active targets at 1,000;
clearing an entry is always allowed at the bound. Per-pair tombstones and retry
history remain durable. The owner snapshot revision advances only when state
changes. Private records are excluded from public export/import routes.

## UI Integration

Keep the approved rounded horizontal card system. Add an author-controls action
on cards and profiles, plus a Blocked and muted management view in the account
menu. Use the existing dialog language, spacing, lucide icons, visible loading,
empty/error/retry states, focus restoration and reduced-motion behavior. Show a
confirmation for blocking, explaining its actual scope. Nothing auto-submits on
navigation or sign-in. Changing accounts clears private state and invalidates
pending responses. Feed filters must apply before publishing rendered results;
cached return navigation must not resurrect hidden posts.

The host waits for the current account's snapshot before admitting feed and
conversation requests. Superseded reads wait for their replacement. A failed
refresh clears recommendation and conversation caches; a later successful
refresh reloads even if its revision is unchanged. Profile/Object visits remain
explicit public reads rather than recommendations. Signing out clears private
snapshots, cached replies, quotes and suspended navigation.

The client uses generated `node.SafetyState`, `node.SafetySnapshot` and
`api.SetSafetyRequest` contracts, with bounded runtime validation at the network
boundary. These REST-only types do not add an embedded Object capability.

## Verification

Current source passes 26 focused backend safety tests and 495 tests across the
affected graph/store/node/API/schema crates. The backend's final host trace is
`backend/crates/node/tests/artifacts/safety/final-regression-host.fozzy`, run
`e346c46d-5d91-43e3-9da1-d92caa3dd9c5`. Its source/export hash manifest matches.

The integrated host passes 753 frontend tests, Astro check (zero errors and
warnings; two existing hints), and production build in
`artifacts/social-safety-ui-transitions-host.fozzy`, run
`9a7a3d99-4472-4a01-a922-0dfbfe948c79`. All 145 SDK tests, generated-contract
freshness and independent fixture conformance also pass. Both host traces pass
strict verification, replay and CI.

The real API/Astro/Aegis focused run also passes in
`artifacts/social-safety-browser-verified-host.1.fozzy`, run
`b499389e-aeeb-4b37-8aad-ec90f916487c`, with strict verification, replay and CI.
It exercises card/profile/account entry points, cancellation without writes,
four-lens filtering and restoration, explicit profile/Object reads, blocked
reply/quote previews, mute-only writes, bidirectional block enforcement through
social invocations, reactions, following and REST/RPC graph writes, independent
unblock/unmute, CAS and exact receipt replay, owner isolation and no-store.
Production-page iframe layouts at 320/390/1280 fit the padded rounded dialogs,
their text and 44px controls. This is not screenshot approval or physical-touch
testing. The parent login and original feed survive test cleanup.

Public export exclusion is covered by the backend tests, not this ordinary-user
browser run. Account-switch, late-response and ambiguous-write retry behavior
has controller/integration tests but no equivalent browser fault injection here.
Replay checks recorded observations; it does not rerun the live services.

The complete real API/Astro/Aegis regression passes in
`artifacts/live-stack-social-safety-host.fozzy`, run
`4a1cb334-4827-4899-ab8a-80d037c51b01` (316 seconds), with strict trace verification,
replay and CI passing. This executes safety alongside publishing, media albums,
reply/share, profiles, conversations, Following, device preferences, reactions,
account security, signed Surface execution, permissions and invocation consent.
The final frontend/SDK/browser sources are hashed in
`artifacts/social-safety-source.sha256`.

The normal API runs the verified backend against the existing store; its saved
identity is unchanged. Aegis confirms the preview on port 4321 is online with
nine Objects, 32px rounded primary cards, loaded safety styles and author/account
controls, and no Astro/Vite error overlay.

Real-browser testing exposed a rapid-toggle bug in the shared panel transitions:
closing panels remain in layout briefly, so checking only the delayed `hidden`
attribute misread their intended state. Toggles now reverse the pending exit
timer. A production-function regression checks the actual visibility helper;
the live account-menu workflow subsequently passed without inserted delays.
Other retained failed traces exposed harness assumptions: fixture ranking must
not assume the first card, and discovery filtering must exclude the author across
the deck rather than assume exploration returns no other authors. The passing
suite navigates to the real fixture and checks every rendered feed author.

Fozzy doctor/test and 32-run scripted fuzz cover the process contract, not native
algorithm or browser fuzzing. Actual execution is in the host traces above.
Distributed `explore` does not accept this steps scenario. A shrink attempt used
the scripted backend and stopped on an undeclared subprocess rather than
reproducing the original setup failure; its output is not a valid minimized
application regression. The original failed host traces remain the evidence.

Cover signatures, CAS/idempotency, lost acknowledgements, changed-intent retries,
restart, owner isolation, public export exclusion, active limits, malformed input,
block enforcement through real write entry points, unblocking, mute-only writes,
private snapshot refresh, and account-switch/late-response suppression. Verify
the real API/Astro/Aegis user workflow and unchanged rounded-card regressions.

Reporting, moderator review, policy-versioned decisions and appeals are a
separate unfinished workflow. These controls do not claim to implement it.
