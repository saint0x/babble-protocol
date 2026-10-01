# Reporting and Moderation

Implementation contract, September 30, 2026. This document describes the intended
workflow; completion is established by the evidence section, not by this design.

## Boundary

Reports concern an existing public Object. Reporting alone never changes its
distribution. Private mute/block and personal Lens preferences remain separate.
Node-local reviewers are explicitly configured by `BABBLE_MODERATOR_IDS` (comma
separated canonical identity IDs). Ordinary account sessions authenticate reviewers;
no operator secret enters the frontend and no account can grant itself authority.
An empty configuration grants nobody review authority. Malformed configuration must
fail closed. Reviewers cannot decide their own reports or cases about their Objects.
An appeal must be reviewed by someone other than the original decision maker.

Reports, appeals, receipts and audit records are private node state, not public
Objects, graph edges, events, exports, logs or Surface bridge data. Reporter identity
and report details are visible only to the reporter and authorized reviewers.
Affected authors see decisions on their Objects, not private report details.
Appeal details are visible only to the appellant and reviewers.

## Workflow

1. An authenticated account submits an Object ID, integrity reason and explanation.
2. Authorized reviewers receive a paginated queue and inspect the Object separately.
3. A reviewer issues `no_action` or `restrict`, with a policy version, reason,
   explanation and optional existing Judgment IDs as source signals. A model score
   never creates a decision by itself.
4. A reporter may appeal `no_action`; an affected author may appeal `restrict`.
   One appeal is permitted per case. Existing restrictions remain during appeal.
5. A different reviewer resolves the appeal with an explained final decision.

`restrict` excludes the Object from node discovery, search, Following and normal
reply projections. Quoted previews retain the signed relationship but omit the
restricted target's content. It prevents new Surface preparation/start and
continued execution on this node. Existing signed
history and explicit public Object reads remain intact. Multiple cases compose:
any effective restriction keeps the Object restricted. `no_action` on appeal
removes only that case's restriction. It is not a deletion or a federation-wide ban.

All mutations use bounded idempotency keys. Changed intent with a reused key is a
conflict; exact retries return the original result. Decisions and appeals require
`expected_revision`. Writes, audit and effective state commit atomically and survive
restart. Lists use stable descending creation sequence pagination, not offsets.

## Host REST Contract

All endpoints authenticate an account, are `no-store` on success and failure, and
are unavailable to the Object/Surface bridge. No equivalent public RPC is added.

- `GET /moderation/access`: `{actor_id, can_review, policy_version, reasons}`.
- `POST /moderation/reports`: `{object_id, reason, details, idempotency_key}`.
- `GET /moderation/reports?scope=mine|affected|queue&before=<sequence>&limit=25`:
  `{items: ModerationCase[], next_before: number|null}` (maximum limit 100).
- `GET /moderation/reports/{id}`: authorized case detail.
- `POST /moderation/reports/{id}/decisions`:
  `{outcome, reason, explanation, policy_version, source_signals,
  expected_revision, idempotency_key}`.
- `POST /moderation/reports/{id}/appeals`:
  `{details, expected_revision, idempotency_key}`.

Reason values: `spam`, `malware`, `fraud`, `harassment`, `illegal_content`,
`other_integrity`. Policy version: `babble.integrity.v1`.

`ModerationCase` fields: `id` (`report_` + 64 lowercase hex), `sequence` (positive
safe integer), `object_id`, `subject_author_id`, `reporter_id` (nullable when
redacted), `reason` (nullable when redacted), `details` (nullable when redacted),
`created_at`, `updated_at`, `revision` (positive safe integer), `status`
(`pending|decided|appealed|closed`), `decisions` (0..2), `appeal` (nullable).

Decision fields: `reviewer_id`, `outcome` (`no_action|restrict`), `reason`,
`explanation`, `policy_version`, `source_signals` (Judgment IDs), `created_at`.
Appeal fields: `appellant_id` and `details` (both nullable when redacted),
`created_at`. Decisions are visible to both parties. Affected-author access begins
only after a decision. Unrelated accounts cannot enumerate or retrieve cases.

Details and explanations must contain 20..4000 Unicode scalar values after trim;
transport bounds must also limit UTF-8 bytes. Signal IDs are unique, at most 20,
must exist and concern the reported Object. Idempotency keys follow the existing
account safety convention. New reports are bounded per account; resolving and
appealing existing cases must not be blocked by report intake limits.

## Ownership and Verification

- Backend agent: domain/persistence/enforcement/API/auth/schema and native tests.
- UI agent: moderation client/controller/views/styles and focused frontend tests.
- Acceptance agent: real API/Astro/Aegis scenario and Fozzy host harness.
- Parent: main application wiring, SDK generation, contract review, combined
  validation, documentation and running preview.

The backend owns canonical generated schemas. The UI must not invent authority,
success, or enforcement. Account switches discard private state. Lost responses
retry the same intent; ambiguous writes do not silently create another report.

## Reviewer Setup

Create reviewer accounts through normal registration and retain their identity IDs.
Set `BABBLE_MODERATOR_IDS` on the API process to a comma-separated list of those exact
IDs, with no spaces or duplicates, then restart that process against the same store.
Use at least two independent reviewer accounts so an initial decision can be appealed
to a different person. Do not put this setting in Astro public environment variables.

Reviewers sign in normally and open **Reports and moderation** from the account
menu. The server's access response controls whether the reviewer queue is available;
that UI response is not authority to submit a decision. Each request checks current
server authority and case eligibility again. Removing an ID from the configuration
and restarting removes review access without discarding historical decisions.

Without configured reviewers, accounts can submit and read their own reports, but
nobody can decide them. Registration never makes a user a reviewer, including the
first account. The development preview does not silently grant the saved account
moderation privileges. Test reviewers belong only to disposable acceptance stores.

Private moderation data lives under `private_moderation` inside the existing node
store. Preserve it with the rest of the store. Running multiple independent serving
nodes against one store remains unsupported. Restrictions are node distribution and
execution decisions, not deletion of the public Object, blob removal, or a claim of
comprehensive illegal-content handling for a public deployment.

## Evidence

September 30, 2026: backend, host UI, generated contract checks, focused real
browser acceptance and the separate combined live-stack regression pass.

- `backend/artifacts/moderation/final-host.fozzy` records 26 focused native
  tests, including reply/quote projections, private storage/restart, independent
  review and appeal, and live bundle gateway retirement. The adjacent regression
  trace `regression-final-host.fozzy` passes 81 tests. Parent verification of the
  recorded source hashes, strict trace checks, replay and CI passes for both.
- `artifacts/moderation-ui-host.fozzy`, run
  `beb225c0-696d-4811-bade-18cb9266e510`, passes the complete frontend test suite,
  Astro check and build. Check has no errors or warnings and two existing hints.
- `artifacts/moderation-sdk-host.fozzy`, run
  `f5f5558d-c307-4192-8d75-984a631b8bff`, passes generated contract checks,
  SDK build and 146 tests. Independent Python fixture conformance also passes.
  Both client traces pass strict verification, replay and CI.
- Independent review caught Following cursors depending on unrelated private
  report activity. They now depend only on effective restrictions. A regression
  test covers reports, decisions, appeals and composing cases.
- A pre-existing SDK timing fixture advanced timers without advancing its
  deadline clock. Its clocks are now synchronized, with a regression test;
  production deadlines and timeout assertions were not weakened.
- `artifacts/moderation-browser/live-host.fozzy`, run
  `69ae0813-b733-48ab-b5a6-b91925d785ac`, records real API/Astro/Aegis acceptance
  and passes strict verification, replay and CI. Reports, decisions and appeals
  are submitted through the production UI with independent API readback. Coverage
  includes redaction/role boundaries, cancelled drafts, focus return, exact retry
  and stale-revision behavior, real API restart, distribution/runtime enforcement,
  and child-comment restriction/reversal with same-page reply and quote refresh.
  Responsive production-page geometry passes at 320/390/1280 widths. Test accounts
  and reviewer roles exist only in disposable stores; the normal account is not
  modified or granted authority.
- The separate full social-platform regression passes in
  `artifacts/moderation-browser/full-stack-host.fozzy`, run
  `3a1baec1-93bb-4b3b-9758-4db850ccf2ad` (361.676 seconds), also strictly
  verified, replayed and passing CI. It checks existing publication, media,
  profiles, conversations, safety, preferences, account and Surface workflows
  against the integrated source. The focused moderation journey is separate,
  not silently counted as part of that full-stack script.
- `artifacts/moderation-browser/source.sha256` records 198 client, fixture and
  harness files. Hash checks pass after both runs. Disposable test processes and
  stores were cleaned up. The normal API was restarted against its existing
  store; its saved public identity is unchanged and no reviewer role was granted.
- Final Aegis verification of the normal preview at port 4321 shows Online,
  nine Objects, loaded host-action and moderation styles, report entry points,
  32px rounded primary cards, and no Astro/Vite error overlay. This confirms the
  pasted missing-`host-actions.css` error is no longer present in the served page.

These checks do not establish semantic moderation quality, physical-device UX,
public deployment readiness, or completion of the broader platform goal.
