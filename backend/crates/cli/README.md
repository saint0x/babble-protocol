# Babble CLI

Build, sign, publish, and inspect Babble objects from local manifests. Run from the
backend workspace with `CARGO_INCREMENTAL=0 cargo run -p babble-cli -- <command>`.
`babble --help` lists commands.

## Already-Built Bundles

The CLI captures explicitly listed output files. It does not compile source,
discover imports, rewrite HTML/JS/CSS, or run a bundler. Compilation and dependency
discovery remain separate work. Include every file your built document references,
keeping its relative paths intact:

```json
{
  "kind": "babble.text",
  "schema": "babble.schema.text.v1",
  "payload": {"text": "My application", "metadata": {}},
  "surfaces": [{
    "role": "Feed",
    "target": "Web",
    "bundle": {
      "entry_path": "app/index.html",
      "files": [
        {"path": "app/index.html", "file": "dist/index.html", "media_type": "text/html", "kind": "document"},
        {"path": "app/main.js", "file": "dist/main.js", "media_type": "text/javascript", "kind": "script"},
        {"path": "app/site.css", "file": "dist/site.css", "media_type": "text/css", "kind": "stylesheet"}
      ]
    }
  }]
}
```

`file` is a local filesystem path resolved relative to the manifest, or an absolute
path. `path` is the canonical, case-sensitive logical bundle path. Bundle kinds are
`document`, `script`, `stylesheet`, `asset`, and `wasm`. The entry must be an HTML
document. The shared object contract validates paths, MIME/kind pairs, and limits.

Bundle input is local-only. Signed inventories with `source_uri`, `integrity`,
`size_bytes`, or `version` are output, not accepted as bundle authoring input.
Bundle surfaces cannot also specify legacy `entry`, `path`, or `integrity` fields.
The CLI computes hashes, sizes, blob URIs, and surface entry from captured bytes;
files are sorted by logical path. Legacy resource/surface inputs still work, and
declared integrity must match any supplied local file.

Limits: 256 files per bundle, 8 MiB per local artifact, 32 MiB of captured local
inputs across the complete build, and 256 KiB for input JSON and each canonical
bundle manifest. Repeated references to the same canonical local file share one
capture. Bundle logical bytes, including repeated content, are also bounded by
the shared contract. Empty files are accepted when their metadata is valid.

```sh
babble build manifest.json --out draft.json
babble identity new identity.json key.json Application my-app
babble sign identity.json key.json manifest.json --out object.json
babble publish ./store identity.json key.json manifest.json
babble inspect bundle ./store <object-id> Feed
```

Each build/sign/publish invocation captures its inputs once. Publish uploads those
exact bytes before publishing the signed draft and reports all uploaded hashes.
Capture bytes stay in memory and are omitted from JSON. Build output contains the
inline bundle inventory; the inventory becomes signature-covered when signed.
Separate invocations capture the then-current files. Build output is not a frozen
artifact archive and is not a replacement input manifest.

Bundle inspection calls the node verifier, which resolves the author's signing
identity at publication time, verifies the object and every materialized bundle
member, then reports the manifest hash, paths, hashes, and verified sizes. It does
not fetch HTTP sources or admit browser execution. Browser execution uses the
API's explicitly configured verified bundle gateway; without it, bundle surfaces
remain policy-blocked.

## Validation

`CARGO_INCREMENTAL=0 cargo test -p babble-cli` covers the actual CLI binary and
captured-input regression. From the repository root, run strict Fozzy doctor/test
on `tests/bundle-inventory.fozzy.json`, then record the actual native suite with
`tests/bundle-inventory-host.fozzy.json` and host backends. The former checks
orchestration only; the latter executes the contract, store, node and CLI tests.
