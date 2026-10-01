# Application Authoring

The Astro composer accepts a built Web application folder alongside its Object
text. It publishes a signed Feed Surface with an inline bundle inventory through
the authenticated Object RPC, then opens the confirmed Object in the swipe deck.
The node must have its [verified gateway](verified-bundles.md#local-gateway-operation)
configured to execute the application. Publication itself does not grant execution.

The composer uses a focused post/reply/share layout with the current author,
attachment tools, image preview/removal and a built-app entry selector. Cancelling
the native image picker keeps the current attachment. Replacing or removing a
preview revokes its local blob URL without discarding the draft text. Attachments
and publication stay owned by the existing account-scoped draft controller;
the presentation layer does not create a separate publishing path.

Published image cards open in a full-image dialog with fit, zoom, mouse panning,
native scroll, keyboard controls and explicit loading/retry states. Closing returns
focus and leaves the reading position intact. Tapping an image opens it; horizontal
swipes on it retain normal deck navigation. This is image viewing, not editing,
video support or completion of all rich-object types.

## Input Contract

- Select one folder containing portable HTML, JavaScript, CSS and assets. The
  selected root is removed once; relative subdirectories are preserved. Choose
  an HTML entry from the entry-document selector.
- Every selected file must use a supported filename extension and canonical
  URL-safe path. Browser-reported MIME is not authoritative. Duplicate paths,
  traversal, ambiguous file/directory aliases, WASM and unsupported types fail
  before upload. Limits are 256 files, 8 MiB per file, 32 MiB total and 256 KiB
  for the canonical manifest.
- File metadata, actual streamed lengths, conflicting content aliases and request
  sizes are checked before uploading. Files are captured once for each attempt.
  Uploads are sequential, and each returned receipt must have matching MIME and
  size, a canonical hash and its matching blob URI. The trusted server computes
  BLAKE3; the browser's SHA-256 alias check is not independent protocol verification.
- No source compiler or dependency discovery is implied. Include the built files
  needed by the application. Executable inline scripts/styles, external runtime
  imports, workers and other unsupported constructs remain subject to gateway CSP;
  the composer does not rewrite them or prove complete-program portability.

## Ownership And Retry

Attachments stay in memory with their account- and origin-scoped draft. Closing
and reopening preserves the immutable Files and entry choice. Reloading does not
restore files from disk, and file contents/names are never saved to local storage.
Selecting an image replaces the application attachment and vice versa. Replies
and shares retain their existing text publication flow.

Every upload and final signed publication checks the original account session.
Repeated submits share the pending draft, and retries of the same draft/payload
reuse its durable publication key. A changed draft or account cannot be cleared
or navigated by an older completion. Upload failure stops before Object publication;
already uploaded content-addressed blobs can remain without a published Object.
Blob garbage collection is a separate operational requirement.

## Declared Permissions

An attached application exposes an App permissions section. Clipboard and
fullscreen have native checkbox controls. Advanced declarations accepts the
protocol's capability array, including scoped and custom-namespace declarations:

```json
[
  { "id": "babble.storage.local", "version": 1, "scope": { "namespace": "my-app" } },
  { "id": "babble.clipboard.write", "version": 1, "scope": {} }
]
```

The common controls preserve unrelated declarations. The raw editor text stays
with the in-memory, account-scoped bundle draft, including invalid intermediate
edits. Closing/reopening retains it; changing accounts hides it; attachment
removal clears it. Neither declarations nor attachment data enter local storage.
Controls lock during publication. Invalid JSON or structural declarations block
publication rather than reverting to an older valid set or publishing without
permissions. Backend errors retain the editable draft for correction and retry.

Before any upload, the publisher captures and validates at most 64 declarations,
64 KiB UTF-8 and JSON depth 16. Each declaration has exactly `id`, positive u32
`version`, and an object `scope`. Duplicate declarations/JSON keys, invalid
Unicode and numbers that would change through JavaScript serialization are
rejected. These are structural authoring checks; the backend remains authoritative
for semantic scope validation, registered capabilities, policy and runtime
admission. A semantic rejection can occur after content-addressed file uploads.

The captured declarations enter the signed Object and its publication retry key.
Declaring a permission grants no authority and does not install an executor.
Viewers still explicitly review access. The Astro host supports real clipboard
writing and fullscreen entry with additional one-time browser confirmation; see
[browser action confirmation](permissions.md#browser-action-confirmation).
Other external capability executors and complete rich-object authoring remain
separate requirements.
