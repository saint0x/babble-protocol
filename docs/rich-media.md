# Rich Media

The Astro composer publishes images, audio, video, and ordered mixed-media albums
as signed media Objects.
The same attachment draft, authenticated upload, publication retry keys, and
account ownership checks used for images apply to audio and video. Closing the
composer keeps the attachment in memory. Signing out does not expose that draft
to another account. Attachments are not written to local storage.

## Albums

The media picker accepts multiple files and appends later selections. The
attachment strip previews, reorders, and removes individual files; the first file
is the published primary resource. The same ordered album works for posts,
replies, and shares. Application bundles and media albums are mutually exclusive.

Drafts and in-flight submissions hold immutable file-order snapshots. Uploads run
sequentially to bound buffering. Publication starts only after every upload has
succeeded; duplicate content hashes are rejected before creating the Object.
A failure retains the entire editable draft. Already uploaded content-addressed
blobs may remain, but they are not partial posts. Retries use the existing
account-bound mutation keys and recheck authorization before each request.

Albums have explicit previous/next buttons, attachment selection, and an ordinal.
These controls navigate within the album; horizontal post swipes still navigate
the feed. Replies and expanded parent context use compact galleries. Selecting
another attachment pauses and releases the outgoing player. Images open the
existing full-image viewer, and audio/video retain native controls and errors.
The resource resolver preserves signed resource order and does not substitute
another resource when an explicit primary is invalid.
For `babble.media`, album order and membership come from `payload.resources`;
outer Object resources may also describe unrelated Surface assets and do not
reorder or extend that album. Generic Object kinds retain their resource fallback.

## Playback

- Images retain the full-image viewer and rounded primary card.
- Video occupies the primary card with native inline controls. Audio uses a
  title and native transport controls within the existing rounded card layout.
- Author context, actions, and compact conversation rows remain below the media.
  Media in incoming replies and parent context renders with the correct player.
- Nothing autoplays. Starting a player pauses other host-page media, including
  composer previews. Swiping away, scrolling the player out of view, hiding the
  page, closing parent details, or removing its view pauses it.
- Removed players release their resource and decode state. Cached conversation
  views restore the resource when reattached, without automatically playing.
- Browser decoding errors remain visible and offer a real reload. A recognized
  container does not guarantee that every browser supports its encoded codecs.

## Replies And Shares

The reply and share composers accept the same image, audio, and video attachments
as a new post, with an optional caption. Application bundles remain a new-post
attachment. Each target has its own draft and preview; closing the composer keeps
the file in this tab. Failed uploads and publications keep the draft available
for retry with the same mutation keys.

The browser uploads the blob through the authenticated media endpoint, then
submits `media: { title, resources }` alongside `text` to
`babble.social.reply.v1` or `babble.social.share.v1`. The existing target-scoped
controller and grant authorize the operation. The node verifies canonical blob
references, integrity, and size before publishing the media Object and its
`reply_to` or `quotes` edge together. Publication receipts make retries durable
across restarts; reusing a key for different content conflicts. Reference metadata
counts toward the social request quota; previously uploaded bytes are not charged
again as social request bytes. This is not cumulative invocation quota accounting.

Replies refresh the current conversation without replacing the parent card.
Shares appear as a new media card with the original Object linked in the graph.

## Delivery

Local resources resolve to `GET /objects/{object_id}/media/{hash}`. This replaces
eager hex-to-data-URI downloads in feed construction. The route derives MIME from
the signed Object resource and requires its exact local blob URI and integrity.
An unpublished blob, missing Object reference, conflicting MIME, unsupported
type, corrupt blob, or oversized blob cannot be served through this route.

`GET` supports a single byte range, including suffix and open-ended ranges.
Successful ranges return `206`, `Content-Range`, and the exact byte length.
Invalid, multiple, or unsatisfiable ranges return `416`. `HEAD` returns full
representation headers without a body and ignores Range. A strong ETag is
provided; mismatched, weak, or date-based If-Range values return the full file.
Responses use `no-store`, `nosniff`, and a restrictive CSP. Full blob integrity is
verified before returning any range. This is bounded buffered delivery, not an
unbounded upload or streaming service.

External HTTP(S) media URLs still load directly from their declared origin.
Their availability and bytes are not verified by this local delivery endpoint.
The declared primary resource is preferred over an incidental image thumbnail.

## Shared-Post Context

Shares display their original posts in smaller rounded previews below the main
card and above its conversation. These previews do not mount embedded apps or
audio/video players. Opening an original uses the full card renderer; back
navigation restores the feed and reading position. Nested visits and profile
detours preserve their respective return histories. Changing accounts clears
that history and cached quote panels.

`babble.social.quotes.list.v1` and `GET /objects/{id}/quotes` return paginated,
deduplicated outgoing quote links verified against the source author's historical
signing identity. Unrelated authors' assertions and unsigned/forged links do not
become a post's quoted context. Missing originals remain explicit unavailable
rows with retry, rather than disappearing or being synthesized. The frontend
requests only the active card's context, ten results per page, with cancellation,
bounded panel caching, and explicit retry/load-more controls.

## Limits

The composer accepts up to 12 attachments totaling at most 64 MiB. These are
client authoring limits, not new protocol resource-count restrictions.
Each image is limited to 4 MiB; audio and video to 8 MiB. The current JSON
upload transport and backend bounded reads support these limits. The picker
recognizes JPEG, PNG, GIF, WebP, AVIF, BMP, ICO; MPEG/MP3, MP4/M4A, AAC, Ogg, WAV,
WebM, FLAC audio; and MP4, WebM, Ogg, QuickTime video. SVG and executable formats
are excluded from inline media delivery. There is no server transcoding.

This does not yet add caption-track authoring, large resumable uploads,
or recording from camera/microphone. Those remain separate functionality, not
implied by native playback.

## Verification

`tests/media-albums-browser.mjs` exercises real multi-file composition, correction
of duplicate content, publication/reply/share, exact ordered resource readback,
binary delivery, gallery controls, image viewing, video playback/cleanup, and
responsive full/compact galleries and composer. Use `BABBLE_LIVE_FOCUS=albums`
with `tests/live-stack.mjs` for the focused journey; the complete suite includes
it by default. Fozzy entrypoints are `tests/media-albums-browser.fozzy.json`
(orchestration) and `tests/media-albums-browser-host.fozzy.json` (real stack).

Album unit tests cover immutable draft snapshots, in-flight account changes,
sequential upload failures, retry keys, attachment limits, resource order and
membership, native-player cleanup, focus, and keyboard navigation. Backend tests
include 13-resource albums, signed metadata and blob validation, missing/corrupt
resources at each position, no partial publication on rejection, restart, and
atomic recovery. Current run evidence is in [production readiness](production-readiness.md).

`frontend/tests/media-resource.test.mjs`, `media-player.test.mjs`, and the composer
tests cover resource selection, validation, native-player state, draft ownership,
and resource cleanup. API media tests cover Object binding, integrity, MIME,
Range/If-Range/HEAD, and real HTTP response bodies.
Node and API social-media tests cover signed media replies/shares, exact graph
links, invalid attachment rejection, scope checks, and durable publication retries.

`tests/rich-media-browser.mjs` extends the isolated API/Astro/Aegis suite with real
WAV and WebM publication, preview, decoding, seeking, video playback, swipe
pausing, narrow-layout checks, and media replies/shares with persisted graph links
and byte-for-byte delivery checks. This suite requires `ffmpeg` with the `libvpx`
encoder to generate its small WebM fixture. Aegis does not provide trusted user
activation for audio play and its installed Chromium build lacks H.264 decoding;
these are explicit browser-verification limits. Layout measurements are not physical-device or
screenshot-based design approval. See the current run evidence in
[production readiness](production-readiness.md).
