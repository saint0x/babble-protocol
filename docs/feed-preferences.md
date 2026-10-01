# Device-Local Feed Preferences

The account menu's **Settings / Feed controls** edits the real local model used
by the Astro host. It is available to guests and signed-in accounts. The existing
rounded swipe deck remains the primary feed; preferences are a separate panel.

## Behavior

- Interests and expertise influence client-side reranking of public candidates.
- Hidden and muted words both exclude matching primary feed candidates. Matching
  uses lowercase, NFC-normalized Unicode tokens, including single characters,
  separated by punctuation and whitespace. It is not substring, phrase, semantic,
  or language-aware word segmentation. For example, `art` does not hide `earth`.
- Hidden author IDs exclude that author's candidates. **Hide author from feed**
  in a post's Object actions persists immediately and offers Undo. Undo removes
  only that addition and retains unrelated intervening preference changes.
- The four ranking sliders and creator affinity adjust the local model. A checked
  **Use Lens default** stores `null`; zero is an explicit override, not a default.
- Following uses the same explicit word/author exclusions while preserving its
  authenticated chronological order. Interests, affinity, and ranking sliders
  do not reorder Following.
- Text, titles, complete descriptions/captions, alt text, kind, and public topics
  participate in word matching. Opaque payload JSON and unrelated metadata do not.
- Filtered-empty results offer Feed controls. Following can still load subsequent
  pages when the loaded page is filtered out.
- Resetting defaults requires confirmation and retains reading history. Clearing
  reading history also requires confirmation, retains saved preferences and the
  session, and does not discard unsaved form edits. New card visits resume history.

Filters apply to ranked and Following feeds, not explicitly opened public
profiles, original/quoted Object visits, comments, or a directly acknowledged
publication. These are **not** protocol blocks, private graph mutes, content
deletions, reports, or moderation Judgments. An author is not notified.

## Storage and Concurrency

Preferences use `babble.local:` followed by the JSON tuple
`[API origin, identity ID or null, "preferences"]` in localStorage. Reading
history uses the same tuple with `"seen"`. Guest, account, and node-origin data
are separate; signing out does not delete either account's preferences.

The parser validates the complete model before writing. It enforces canonical
author IDs, finite `0..1` scores, bounded string arrays, at most 128 distinct words
per term field (including words within phrases), and a 64 KiB UTF-8 limit.
Malformed saved preferences remain intact and produce a visible warning instead
of being silently deleted. Storage denial/quota errors cannot report a successful
save or hide. The user can explicitly replace invalid preferences through Apply
or Reset after reading that warning.

Saving checks the displayed account and preference snapshot. A detected change
from another tab requires reopening Feed controls before saving; same-session
account metadata refreshes do not reset drafts. Storage events refilter the feed
without replacing unsaved inputs. History-only events update the count without
triggering a cross-tab feed-refresh loop. localStorage does not provide atomic
cross-tab transactions; the snapshot check detects prior changes but is not a
distributed lock against simultaneous writes.

The model and reading history are consumed in the browser, not sent with public
discovery or Following requests. The SDK's encrypted sync contracts are separate
and are not automatically invoked by these controls.

## Implementation and Verification

- `frontend/src/app/local-preferences.ts`: validated storage boundary.
- `frontend/src/app/feed-preferences.ts`: account scope, saves, reset, clear, Undo,
  and cross-tab notifications.
- `frontend/src/app/preferences-view.ts`: accessible tabbed editor, confirmation,
  preserved drafts, and errors.
- `frontend/src/app/main.ts`: feed integration, local reranking, immediate loaded
  card filtering, history, and navigation invalidation.
- `sdk/src/personalization.ts`: shared filter and public-content projection.

Focused unit suites cover validation, storage failures, account transitions,
stale form saves, Undo, form confirmation, Unicode/full-caption matching, and
production-main integration. `tests/preferences-browser.mjs` runs the live
account/Settings journey within the isolated API/Astro/Aegis stack. See
`production-readiness.md` for recorded results and limitations.

For a focused live check, use `tests/feed-preferences-browser-host.fozzy.json`
with Fozzy's host process/filesystem/HTTP backends. It selects
`BABBLE_LIVE_FOCUS=preferences` in the same live-stack runner; the default runner
still exercises the full platform. Build the SDK before running browser tests,
and do not rebuild shared dependencies while a live browser suite is active.
