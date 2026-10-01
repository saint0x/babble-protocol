# Verified Executable Bundles

**Status: native inventory, publication, materialized-byte verification, and the
local gateway/SDK execution path are implemented.** A Surface can carry an inline
signed manifest. The CLI captures already-built artifacts once, publishes those
bytes, and inspects a complete verified immutable snapshot through the node's
historical signing-key resolver. See [the CLI contract](../backend/crates/cli/README.md).
The API can start these bundles through an explicitly configured loopback gateway.
The SDK requires its session-bound mount descriptor and never falls back to the
source URL. Legacy URL Surfaces do not acquire these guarantees.

Remote acquisition, dependency compilation, public gateway provisioning, and hard
browser resource enforcement remain implementation requirements. The verifier checks
every declared file, not whether a program's complete dependency tree was listed.
The document-bound MessagePort does not authenticate the first document's bytes.

## Decision

Use a signed path manifest, portable build output, complete materialization into
verified storage, and a gateway on a separate origin for each mount. External
HTTP(S) resources remain supported as integrity-pinned acquisition sources. The
browser executes the verified snapshot, never refetches the upstream entry.

Production requires a **separate content domain** from the application/API domain,
wildcard DNS/TLS or equivalent isolated-origin provisioning, and gateway routing
that binds each mount origin to exactly one bundle and entry document.

Local preview origin provisioning now has [real Aegis evidence](bundle-origins.md):

- An ephemeral loopback port per mount provides distinct origins, but needs
  listener lifecycle management; cookies are not isolated by port.
- A random single-label `*.localhost` name per mount works on one loopback
  listener in installed Aegis, with secure-context, origin/storage isolation,
  cookie controls, relative imports, CSP and exact-origin/source admission tests.

The local gateway uses the tested hostname strategy. Production
DNS/TLS, parent-domain cookies, mount lifetime and storage cleanup still require
their own implementation and validation. Never fall back to the application
origin or accept opaque `null` origins for this gateway mode.

## Signed Contract

The implemented `Surface.bundle` is a bounded inline manifest committed directly
by the signed Object. Omitting it preserves legacy canonical bytes and signatures.
The manifest is canonical data with these fields:

| Field | Meaning |
| --- | --- |
| `version` | Bundle format version; unsupported versions fail closed. |
| `entry_path` | One logical path identifying the executable entry. |
| `files[]` | Complete inventory, including declared dynamic imports and assets. |
| `files[].path` | Unique, canonical path relative to the bundle root. |
| `files[].source_uri` | External URL or canonical `babel://blobs/<hash>` source. |
| `files[].integrity` | BLAKE3 digest of the exact final file bytes. |
| `files[].size_bytes` | Exact decoded file size. |
| `files[].media_type` | Canonical MIME fixed by the commitment. |
| `files[].kind` | Validated role: document, script, stylesheet, asset or WASM. |

The Object commitment covers the complete manifest; the verifier also computes
its versioned canonical hash for the receipt. A separate manifest fetch is not
needed. Files must be sorted lexicographically by their unique logical paths.
Keep Surface entry integrity equal to the entry file's digest. Resolve the
Surface entry through an explicit manifest mapping, never a hash substring or
implicit application-origin URL. Permit multiple paths with identical content
hashes; reject duplicate paths, ambiguous aliases and incompatible MIME/kind pairs.

Manifest paths contain no traversal, backslashes or ambiguous encodings. Resolve
relative imports against the importing file's logical directory; `../` is valid
only when its normalized destination stays inside the bundle. Query parameters
cannot select alternate bytes or MIME. Fragments do not identify different files.
Browser gateway URLs are derived transport addresses, not signed build inputs.

## Authoring and Publication

The [Astro application composer](application-authoring.md) also accepts a built
folder, validates/captures its Files, uploads the bytes through authenticated RPC,
and publishes a signed Feed/Web inventory. It does not compile source or discover
dependencies. Browser authoring and CLI capture share the signed manifest contract.

Current build/sign/publish commands accept explicit local, already-built output
files and retain one bounded in-memory capture per canonical source path. Hashes,
sizes, signed inventories and uploaded bytes all come from that capture. Inputs
are not reread during publication. Separate commands capture anew; `build --out`
is a draft, not a frozen artifact archive. Native `inspect bundle` verifies every
materialized file and the Object signature, including historical key rotation.

The remaining compiler work is:

`babel build` must produce portable final bytes before computing resource hashes:

1. Use established HTML/CSS/JavaScript parsers and bundlers to compile dependencies
   into a stable logical layout. Preserve module cycles and relative references.
2. Normalize HTML resource attributes, CSS imports/URLs, module imports and
   supported `new URL(..., import.meta.url)` assets. Resolve bare imports at build
   time or emit one validated import map before module execution.
3. Externalize executable inline scripts. Include explicitly declared dynamic
   imports/assets; reject unsupported build constructs with actionable errors.
   Static analysis is not a proof of every possible computed URL.
4. Hash the emitted bytes, emit the manifest and permission/resource report, then
   sign the Object. Publish those exact artifacts, not reread mutable build inputs.

Final artifacts may live externally or in Babel blob storage. Already-portable
external HTML can remain byte-for-byte unchanged. Nonportable external inputs
must be compiled into newly hashed output; do not transform bytes at serving time
and claim the original digest identifies the transformed executable. Gateway
origin changes must not require rewriting signed files. Updating executable
bytes or the manifest requires a new Object commitment/version.

For the initial Web profile, require an HTML entry; JavaScript/CSS entries need a
build-generated, hashed HTML wrapper. Workers/worklets and browser WASM compilation
remain unsupported until their execution paths receive equivalent verification.
Native WASM admission must also consume verified entry/module bytes.

## Preparation and Execution

1. Verify the Object signature/commitment, manifest digest and structure, entry
   mapping, resource types and aggregate limits.
2. Acquire every manifest file, enforcing exact size and digest. All executable
   bundle files must verify before the bundle becomes ready. Large independently
   streamed media needs a separate integrity contract, not an unchecked exception.
3. Atomically publish a verified snapshot. Only the verifier constructs the
   internal `VerifiedBundle` value. Serve its verified buffers or pinned immutable
   snapshot; never reopen an unchecked mutable path or refetch an upstream URL.
4. Separate policy admission from byte readiness in the runtime plan. Session
   start consumes a verified receipt and rechecks ownership/grants. Bind receipt
   and mount to Object, role, manifest hash and verifier policy version.
5. Allocate an isolated mount origin. Its gateway serves only the entry document
   and declared files, with MIME from the manifest. Unknown paths, alternate
   document navigations and MIME overrides fail. No redirects, API routes,
   directory fallback, arbitrary URL proxy or executable error documents.
6. SDK mount requires the verified mount descriptor. It must not derive an
   executable URL from the original Surface entry or treat a readiness boolean
   from untrusted child code as proof.

Gateway responses use restrictive CSP headers, `nosniff`, no-referrer and
Permissions-Policy. Allow required scripts/styles/assets and asset fetches only
from the exact mount origin. Set `default-src 'none'`, `base-uri 'none'`,
`object-src 'none'`, `frame-src 'none'`, `worker-src 'none'`, `form-action 'none'`
and an exact host `frame-ancestors`. Omit `unsafe-eval`, executable `blob:`/`data:`
and `strict-dynamic`. Hash-authorize a generated inline import map when needed;
CSP/SRI SHA hashes are separate from Babel's BLAKE3 commitments. Additional iframe
policies must be consistent with these headers; iframe attributes are not the
enforcement foundation. Host-mediated network results remain data.

Introduce an explicit gateway sandbox mode using
`sandbox="allow-scripts allow-same-origin"` on the separate mount origin. It is
never the host origin. Admit a MessagePort only from the exact assigned origin
and frame source; retain confirmation-before-RPC, single admission, deadlines,
navigation teardown and session-scoped dispatch. This prevents an initial
external navigation from offering an admissible port. If opaque origins remain
mandatory, a trusted bootstrap must establish the channel before releasing any
untrusted markup/code; that is a different, more complex loader design.

## Acquisition and Transport Limits

Inventory/file/bundle ceilings below are implemented in the native contract.
Network acquisition ceilings remain proposed. Hosts may lower them:

| Limit | Proposed ceiling |
| --- | --- |
| Manifest | 256 KiB; 256 files |
| File | Existing bounded-blob ceiling, or a lower bundle policy limit |
| Bundle | 32 MiB total decoded bytes |
| Acquisition concurrency | Four files per bundle, plus a global bounded queue |
| Deadlines | 10 seconds per resource; 30 seconds for the complete bundle |
| Response headers | 16 KiB, enforced by the transport |
| Redirects | Zero in the initial profile |

Require HTTPS/public destinations in production. Reject URL credentials and
nonpublic/special-use addresses, including IPv4-mapped IPv6. Validate DNS results
and pin the actual connection to an approved address while preserving TLS
hostname validation. Disable ambient proxies, cookies and forwarded credentials.
Any future redirect support must repeat every check at each hop within the same
total deadline. A preview loopback exception must be explicit and restricted to
configured fixture endpoints; it must never enable production private-network
fetching.

Bound DNS/connect/read time, actual body bytes, aggregate memory, temporary disk
and concurrent materializations. Do not trust `Content-Length`. Request identity
encoding and reject unexpected encodings initially; supporting compression later
requires separate compressed/decoded limits. Reject partial upstream responses.
Cancel work and delete incomplete snapshots on failure. Never expose unverified
response prefixes to the browser.

Serve resources through bounded binary HTTP delivery, not large JSON/hex RPC
envelopes. Bound serialized API responses and both MessagePort directions,
in-flight requests and dispatch lifetime. Application validation occurs after
browser structured-clone allocation, so it is not a hard memory-isolation limit.

## Boundaries and Alternatives

| Owner / source | Proposed responsibility |
| --- | --- |
| [Object contracts](../backend/crates/object/src/lib.rs), generated SDK schemas | Inline versioned manifest commitment and runtime mount descriptor are implemented. Reuse canonical URI validation. |
| [Authoring](../backend/crates/authoring/src/lib.rs), [CLI](../backend/crates/cli/src/main.rs) | Portable compilation, inventory, final-byte hashing and publication artifacts. |
| Node materializer and store | Bounded acquisition, verification and immutable snapshots; network work outside the global node lock. Reuse bounded blob reads. |
| [Runtime](../backend/crates/runtime/src/lib.rs) | Policy checks and verified-receipt admission; no HTTP fetching in policy evaluation. |
| [API routes](../backend/crates/api/src/routes.rs) and content gateway | Mount binding, file membership, committed MIME, headers and transport limits. |
| [SDK host](../sdk/src/host.ts), [channel](../sdk/src/channel.ts) | Verified-descriptor mounting, exact-origin admission and existing document-port lifecycle. |

A browser blob-URL/srcdoc loader would require additional handling for module
cycles, relative paths, CSS assets, CSP inheritance and URL lifetimes. Import maps
alone cover document module imports, not script `src`, CSS/assets or workers.
SRI alone does not establish the complete inventory or authenticate an iframe
document. Shared-origin gateway paths are harder to isolate; CSP path restrictions
also weaken across redirects. The selected gateway serves a finite verified
inventory and never redirects. See [import-map behavior](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/script/type/importmap)
and [CSP redirect matching](https://w3c.github.io/webappsec-csp/#source-list-paths-and-redirects).

## Requirement and Acceptance Matrix

The complete execution requirements below are **not all closed**. Local gateway
and native evidence are recorded separately; they do not establish public
deployment, remote acquisition or hard browser-resource guarantees.

| Requirement | Required evidence |
| --- | --- |
| `spec.md` sections 15/16: immutable executable dependencies | Mutating entry or any dependency prevents readiness; undeclared computed imports cannot execute. |
| `spec.md` external integrity resources | External entry and transitive dependencies execute from verified snapshots; browser/upstream logs prove no browser upstream refetch. |
| `sdk-spec.md` section 40: portable supply-chain workflow | Identical signed output works through two gateway origins, with nested relative imports, cycles, dynamic imports, CSS/fonts/images and Babel blobs. |
| `sdk-spec.md` section 41: verify before sandbox/handshake | No mount or capability dispatch before full verification; initial redirect and replacement-document port offers fail. |
| Object-scoped dependency admission | Cross-bundle blobs, MIME overrides, unknown paths and encoded traversal fail; exact declared paths succeed. |
| Browser execution containment | External dynamic imports, inline/eval/blob execution, workers and unsupported WASM paths are blocked by actual browser behavior. |
| SSRF and bounded transport | Private-address attempts, DNS rebinding, redirects, truncation, chunked oversize, unexpected compression, deadlines and exhaustion fail within bounds. |
| Snapshot/TOCTOU handling | Upstream mutation after verification cannot change executed bytes; cache corruption or mutable-file replacement cannot produce unchecked delivery. |
| Deployment isolation | Production content origin differs from host; preview strategy passes real resolution, origin, cookie, secure-context and lifecycle tests. |

Use real Aegis fixtures following [the document bridge regression](../tests/document-bridge-browser.mjs),
on a separately parameterized browser runtime. Assert executed markers, gateway
and upstream request logs, and capability dispatch counts. Include deliberately
weakened negative controls to prove the harness detects missing enforcement.
Do not count fake-DOM or scenario-script checks as browser execution evidence.

Run Fozzy strict doctor (`--deep --runs 5 --seed 42`) and deterministic strict
tests first; then a host-backed Aegis scenario with a recorded trace, strict trace
verification, replay and CI. Use fuzzing/shrinking for manifest, URI and acquisition
failures; broaden scenario exploration where supported. Record actual coverage
and limitations in production readiness before closing this requirement.

## Native Evidence

- Bundle contracts validate strict fields, source URIs, canonical hashes, sorted
  unique logical paths, exact MIME/kind pairs and aggregate bounds. Same-hash
  aliases require identical metadata. Rust/SDK/Python fixtures share the canonical
  inventory bytes; signature tests preserve legacy Objects and reject mutations.
- `FileStore::verify_surface_bundle` consumes already-materialized blobs only,
  checks all declared sizes and hashes, and returns private-construction immutable
  buffers after the whole inventory succeeds. Missing, altered or oversized files
  cannot yield a partial receipt. Later disk changes cannot alter returned bytes.
- Low-level verification takes a trusted historical signing identity. Network or
  user-facing consumers must resolve it authoritatively; `LocalNode` and the CLI
  do so through verified identity transition history. The receipt is not a
  serializable readiness assertion, a capability grant, or browser admission.
- The private local store remains trusted; no hostile-filesystem or hard disk-I/O
  deadline guarantee is introduced. HTTP sources must already be materialized by
  hash; this verifier never fetches them or silently falls back to upstream URLs.
- Actual test and trace evidence is recorded in
  [production readiness](production-readiness.md). The origin experiment validates
  browser mechanisms independently, not this native verifier's browser integration.

## Trust and Residuals

The verifier, gateway, trusted host and TLS delivery are in the trusted computing
base. This proposal does not provide verification against a compromised gateway.
Hash identity does not make code benign, prevent intentional port delegation, or
prove arbitrary program behavior safe. The guarantee covers acquired executable
files and browser loading paths, not arbitrary interpreters written in admitted
code. Normal mount origins expose browser-local storage: cleanup and quota policy
remain necessary. Browser CPU/memory/GPU enforcement is separate work; neither
integrity nor CSP supplies those limits. Unsupported execution profiles must fail
closed rather than silently bypass this contract.

## Local Gateway Operation

Set `BABEL_BUNDLE_GATEWAY_ADDR=127.0.0.1:8788` when starting the API, choosing a
free port. `BABEL_CORS_ORIGINS` supplies the exact allowed parent origins; wildcard,
credential-bearing, path-bearing and noncanonical origins are rejected. The
gateway has a dedicated listener, not an API route or arbitrary URL proxy. Its
current provisioner accepts only `127.0.0.1` and a nonzero port. Do not publicly
reverse-proxy it as a substitute for production content-domain provisioning.

Preparation verifies all files and rechecks identity-scoped capability policy,
returning descriptive `bundle_verification` metadata. Preparation alone cannot
mount an iframe. Authenticated session start verifies again, admits the native
session and allocates a 192-bit random single-label `m-*.localhost` origin. Its
`verified_mount` binds the session, Object, role, manifest hash, origin and exact
entry URL. An idempotent start can return only that account session's existing
mount. A lost or evicted mount requires a fresh session, never a source-URL fallback.

The gateway serves immutable snapshot buffers with committed MIME, restrictive
CSP, `nosniff`, no-referrer, no-store, same-origin resource policy and denied
ambient device permissions. Only the entry can be a document navigation. Unknown
Hosts/paths, percent-encoded aliases, query overrides, alternate documents and
non-GET/HEAD methods fail closed. Native browser WASM and workers are not enabled
by this profile. Header policies also wrap middleware error responses.

Every delivery checks the owning account session, live Surface lease/lifecycle
and current capability grants. Eviction, consent revocation or account revocation
prevents new delivery; the existing host lease controller stops the mounted
frame. Already delivered bytes cannot be recalled. Native snapshots are pruned
periodically and during allocation. Bounds are 64 mounts, 128 MiB of logical
inventory bytes, and 16 in-flight resource bodies; a body retains its capacity
slot until released. Verification remains bounded synchronous local-store work,
not a hostile-filesystem deadline guarantee or remote acquisition implementation.

The SDK accepts only the exact assigned nonopaque origin and frame source, with
`allow-scripts allow-same-origin` on this separate origin. It retains the existing
confirmed MessagePort handshake and navigation teardown. Descriptor checks are
not independent signature/BLAKE3 verification: the authenticated host API and
gateway are trusted. No new public deployment, persistent origin cleanup, browser
storage quota, CPU/GPU limit, or complete-program dependency-discovery guarantee
is implied by this local execution path.
