# Babble Protocol

Babble  is a programmable social information protocol: signed Objects, typed graph relationships, replaceable Judgment providers, user-controlled Lenses, executable Surfaces, algorithmic discovery, and selective decentralized ordering.

Implementation is ongoing. Working local flows are not proof of complete production
readiness; the [readiness ledger](docs/production-readiness.md) tracks verified
milestones and remaining requirements.

Current priority is complete social functionality and a polished, seamless
rounded-card Astro experience, with baseline security and reliability. Advanced
deployment hardening is tracked separately so it does not displace unfinished
user workflows. See [current priorities](docs/production-readiness.md#current-priorities).

The active implementation starts from the specification in [spec.md](spec.md). The original Next.js client remains in the history of [Babble frontend](https://github.com/saint0x/babble-frontend).

## Layout

```text
backend/     Rust workspace for protocol, graph, judgment, state, node/API work
algorithms/  Typed Python/uv workspace for production ranking, discovery, and Judgment logic
sdk/         TypeScript SDK generated from the Rust-derived protocol schema bundle
frontend/    Astro social client for the card-swipe feed, publishing, and browser Surface host
tests/       Fozzy deterministic scenarios and live-stack checks
artifacts/   Local recorded Fozzy evidence (excluded from Git)
fixtures/    Protocol schema bundle and golden conformance values
```

Runtime stores, signing keys, environment files, logs and recorded execution
traces remain local. The scenarios and source tests are versioned; paths to traces
in the readiness ledger refer to local evidence rather than bundled release files.

## Backend Crates

```text
babble-types     Shared identifiers, timestamps, versioned canonical encoding, hashing, errors
babble-api       HTTP/JSON API over the local node
babble-authoring Validated unsigned publication drafts for Objects, edges, and capability grants
babble-cli      Developer workflow CLI for manifest validation/build and local store inspection
babble-capabilities
                Host capability definitions, permission modes, scoped grants, quotas, and broker decisions
babble-crypto    Ed25519 keypairs, public keys, signatures
babble-discovery Graph-backed mixed candidate generation
babble-eval      Judgment, Lens/ranking, discovery, finality, and realtime regression corpus metrics
babble-identity  Signed cryptographic identities
babble-object    Signed Object envelopes, resources, surfaces, capabilities
                Core Object schema registry and typed payload/capability validation
babble-personalization
                Local-only user model, private preference filters, and inspectable
                post-Lens reranking traces for client-side personalization, plus
                opt-in encrypted sync envelopes for explicit cross-device model transfer
babble-graph     Signed typed edges and in-memory graph index
babble-hashgraph Deterministic event DAG, ancestry, virtual voting, finality, and checkpoints
babble-judgment  Provider-independent Judgment definitions for moderation, content analysis,
                relevance, relationships, spam, and evidence quality; typed input/output registry, provider trait,
                privacy-aware orchestration, batching, cache metadata, and provider decision traces
babble-judgment-local
                Deterministic local Judgment provider with publish-time Object analysis and moderation
babble-judgment-python
                Bounded persistent Python worker, versioned wire schemas, and core-owned Judgment records
babble-judgment-jev
                Jev-compatible HTTP adapter behind the Judgment provider trait
babble-lens      Built-in Lens ranking policies, composition, and ranking traces
babble-media     Content-addressed media blob descriptors and media Object payload contracts
babble-network   Versioned gossip envelopes, inventory/request/bundle exchange, bounds checks
babble-node      Local node composition API for authoring, persistence, graph, and Judgment
babble-realtime  Room specs, typed payload registry, sessions, presence, durable messages, and collaborative state reducers
babble-rpc       Versioned RPC method catalog, request/response envelopes, binding validation, and structured errors
babble-runtime   Surface lifecycle admission, resource budgets, sandbox policy, and capability gating
babble-schema    Rust-derived JSON Schema bundle and golden fixture exporter
babble-sim       Deterministic multi-node gossip simulation with adversarial delivery cases
babble-state     Signed events and local in-memory protocol state
babble-store     File-backed protocol record store, opaque encrypted personalization sync vault,
                and content-addressed blob store
```

The current backend milestone covers Phase 1 foundations plus the first provider-independent Judgment layer. Jev-specific code lives only in `babble-judgment-jev`.

## Algorithms

The algorithm layer uses typed Python with `uv`, while the protocol core keeps durable records, signatures, canonical IDs, and schemas stable.

The [algorithm migration audit](docs/algorithm-migration.md) maps all deprecated
modules to current behavior and missing requirements. The served API now defaults
to the local Python worker for seven Judgment definitions. Publication
executes spam, evidence quality, content analysis, and moderation before committing
an Object; the API also evaluates relevance, relationships, and source agreement.
Rust validates
outputs and owns their provenance, commitments, cache, and persistence. Discovery
consumes persisted spam/evidence signals. Rust admits a bounded public candidate
pool; Python executes all eight public Lens rules, blends, diversity adjustments,
and numeric ranking traces through the same worker. Both health and discovery
identify the ranking provider. Private Following stays chronological and private
personalization stays in the browser. The personalized feed requests up to 200
public candidates, applies private filters and preference scoring, reapplies the
canonical soft diversity policy, then selects and hydrates nine cards. No private
preferences are included in the public discovery request. See
[public ranking](docs/public-ranking.md) for the retrieval contract, verification
and remaining limits.
The same worker now executes [temporal scoring](docs/temporal-scoring.md) from one
evaluation clock and signed public relationship activity. On-demand source
agreement has a live inspector and signed public evidence inputs; engagement
analytics still lacks complete live consumers, and temporal policy still needs
empirical evaluation. Explicit searches rank only lexical matches, and an
empty discovery result remains empty in the browser.

These algorithms remain lexical heuristics, not complete semantic-model
implementations or calibrated probabilities. Content/moderation outputs are
explicitly uncalibrated. See the [worker contract and configuration](docs/algorithm-worker.md).
The [semantic integration audit](docs/semantic-integration.md) records the missing
provider connection, definition contracts and independent model-quality gates.

```bash
cd algorithms
uv sync --frozen
uv run --frozen pytest
uv run --frozen basedpyright
uv run --frozen ruff check
```

## SDK

The TypeScript SDK is generated from `fixtures/protocol/v1/schema-bundle.json` and wraps the RPC envelope contract without reimplementing protocol logic. It exposes the generated protocol surface, typed RPC client, HTTP transport, browser message bridge transport/host listener, browser Surface host mounting, prepared-Surface SDK binding, capability-bound SDK namespaces, encrypted personalization sync helpers, and Surface lifecycle helpers.

Social follow/unfollow/share/reply helpers now use `` RPC methods; their old
`.v1` mutations return `UnsupportedVersion` (`UNSUPPORTED_VERSION`). Capability
declarations remain version 1. These four actions use exact, one-use invocation
consent and publication receipts, never reusable social approval grants.
Clipboard/fullscreen helpers also use ``, with one-use dispatch and typed
native outcomes. See the [current contract and five remaining external methods](docs/invocation-consent.md).

```bash
npm --prefix sdk run generate:check
npm --prefix sdk run build
npm --prefix sdk test
```

## Developer CLI

The `babble` CLI is the local developer workflow entrypoint for production Object manifests and node-store inspection. It validates manifests through the same `babble-authoring` domain checks used by the node, computes content hashes for local resources, writes validated draft JSON, previews runtime admission and sandbox diagnostics for declared Surfaces, emulates local dev-host capability decisions, creates local signing identity files, signs manifests into Objects, publishes manifests into a file-backed node store, verifies signed stored Objects, and summarizes graph edges from the file store.

```bash
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- --help
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- validate path/to/manifest.json
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- build path/to/manifest.json --out draft.json
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- preview path/to/manifest.json
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- dev path/to/manifest.json
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- identity new .babble/alice.identity.json .babble/alice.key.json Person alice
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- sign .babble/alice.identity.json .babble/alice.key.json path/to/manifest.json --out object.json
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- publish .babble-node .babble/alice.identity.json .babble/alice.key.json path/to/manifest.json
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- inspect store .babble-node
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- inspect object .babble-node obj_<hash>
cargo run --manifest-path backend/Cargo.toml -p babble-cli -- graph object .babble-node obj_<hash>
```

Manifest resources and executable Surfaces accept either declared `integrity` or a local `path`; when `path` is present the CLI computes the BLAKE3 content hash and rejects mismatches. Local-path executable Surfaces are rewritten to `babble://blobs/<hash>` entries during build/sign/publish, and `publish` uploads every referenced resource and Surface file into the node's content-addressed blob store before signing the Object. `preview` and `dev` also require executable Surface integrity to match a declared Object resource, then report runtime admission, CSP, resource budgets, WASM policy where applicable, capability prompts, and blocked reasons before an author signs or publishes.

For multi-file applications, [local bundle inputs](backend/crates/cli/README.md)
produce a signed inline inventory with exact paths, MIME, sizes and hashes.
`babble inspect bundle <store> <object-id> Feed` verifies every materialized member
through the authoritative historical signing key. Build/publish capture files
once and upload those exact bytes. This accepts already-built outputs. The
[local verified gateway](docs/verified-bundles.md#local-gateway-operation) enables
execution when the API is started with `BABBLE_BUNDLE_GATEWAY_ADDR=127.0.0.1:8788`
(use a free port) and explicit `BABBLE_CORS_ORIGINS` for its parent application.
Without the gateway, bundle execution remains blocked.

## Frontend

[Reporting and moderation](docs/moderation.md) is available from post/reply
controls and the account menu. Private reports lead to explicit reviewer decisions,
author notices, and independent appeals. Reviewers require operator configuration;
registration grants no moderation authority. Node-local restrictions affect feeds,
reply/quote previews and Object execution, while retaining signed public history.

The composer supports [built Web application folders](docs/application-authoring.md):
select an HTML entry, publish its signed multi-file inventory, and open it inside
the rounded feed card. Execution requires the verified gateway configuration
above. Files stay in the account-scoped in-memory draft; compilation and complete
dependency discovery are not yet included.

[Rich media](docs/rich-media.md) supports image, audio, video, and mixed-media
albums for posts, replies, and shares, with ordered attachment editing, native
previews, and playback controls. Album navigation stays separate from post swipes.
Local media uses integrity-checked binary
delivery with byte-range seeking. Playback is opt-in and pauses when you leave
its card. Albums accept up to 12 files and 64 MiB total; each image is limited to
4 MiB and audio/video to 8 MiB. Codec support is
browser-dependent, with explicit errors and retry rather than fake playback.

[Object permissions](docs/permissions.md) can be reviewed from the card's action
menu or Surface toolbar. The host stops local execution before review, submits
explicit scoped approvals/revocations, and reconciles server state before another
change. Grant/revoke retries now persist the original event while returning
current access. The four social `ask_each_time` methods now have a dedicated
host prompt with Allow Once, Deny and cancellation, separate from management;
management shows Ask every time, not durable Allow. Host composer actions use
explicit one-use intent and read-only completed-result recovery. A real
API/Astro/Aegis run verifies the four social actions, exact retries and no-write
denial/cancellation/expiry/closure. Clipboard and fullscreen now share durable
one-use consent with separate native activation. Five other
`ask_each_time` methods still lack durable invocation consent. See the
[invocation contract](docs/invocation-consent.md) and
[scoped evidence](docs/production-readiness.md#social-invocation-milestone).

On-demand [source agreement](docs/source-agreement.md) now connects signed public
evidence to the Python algorithm through the Judgment inspector. Independent
components and exact persisted inputs are inspectable; this is lexical comparison,
not a truth score or a replacement for hashgraph consensus.

The current [design research](docs/design-research.md) separates evidence from
design hypotheses. The client now uses horizontal Object columns with vertically
connected conversations: content-first text/media cards, compact paginated
replies, nested threads with Back navigation, and per-post scroll restoration.
The backend exposes verified direct replies through `babble.social.replies.list.v1`.
These engineering checks do not establish usability or production readiness;
see the [remaining production work](docs/production-readiness.md).

The Astro frontend is a static protocol client. Horizontal navigation changes
Objects; each Object has a vertically scrollable conversation. Images occupy the
primary card, with author context and compact replies below. Executable Web
Surfaces open inside the rounded primary card and can expand without remounting.
Closing restores the post's reading position and focus, removes the executable
iframe, and evicts its backend session. See [inline Surfaces](docs/inline-surfaces.md)
for lifecycle guarantees, verification, and remaining runtime limits.
Host-owned [60-second leases](docs/surface-leases.md) bound abandoned sessions;
renewal keeps the mounted app intact, while failure stops it and offers retry.
The [document-bound bridge](docs/surface-bridge.md) registers one immutable document
per Surface session and login before enabling RPC, then rechecks that binding at
backend execution. It uses one child-created
MessagePort per mount, with confirmed readiness before activation. Legacy
window-RPC Surfaces must be republished; signed content is never rewritten or
silently admitted through a compatibility fallback.

[Resource admission and delivery](docs/resource-delivery.md) enforce canonical
URI/hash matching and digest-verified, 8 MiB bounded local blob reads across raw
HTTP, REST, and RPC. This does not verify external executable dependencies; the
[verified-bundle work](docs/verified-bundles.md) now includes signed inventories,
single-capture CLI publication and native verification of every materialized file.
An explicit local gateway and SDK mount descriptor connect these verified bytes
to isolated browser execution. Dependency compilation, external acquisition,
public gateway DNS/TLS and hard browser resource limits remain unfinished;
bundle Surfaces cannot fall back to the unverified URL host.

Rounded primary cards retain horizontal swipe navigation; narrower conversation
cards form the vertical hierarchy. The active card also exposes
[versioned temporal analytics](docs/temporal-scoring.md) from actual discovery
evaluation, without collecting private reading telemetry, and
[public reactions](docs/reactions.md): independent appreciation, engagement,
position, and optional confidence, with explicit publication consent, durable
readback, conflict recovery, and withdrawal. These are attributed human choices,
not a truth score or calibrated probability.

Publishing uses an authenticated account, with signing keys held by the local
node. Open the account menu to register or sign in. Login uses the full identity
ID shown in the account dialog, not the public handle. Sessions survive page
reloads in the current tab; sign-out revokes them on the server. Account settings
list active sign-ins and allow individual or all-other-session revocation.
Changing a password requires the current password and signs out every session,
including the acting browser. New passwords require at least 15 Unicode scalar
values and at most 1024 UTF-8 bytes; existing passwords remain valid for login.
Legacy local
author IDs and seed identities are not password accounts. See
[accounts and authority](docs/authentication.md) and the
[backend contract](backend/AUTH.md), including recovery and deployment limits.

```bash
cd frontend
npm run dev -- --port 4321
npm run check
npm run build
```

Set `PUBLIC_BABBLE_API_URL` when the API is not running at `http://127.0.0.1:8787`.

## Run

Install the Python worker before running the API or the Rust workspace's
real-worker tests (commands below start from the repository root):

```bash
uv sync --frozen --project algorithms
```

```bash
cd backend
cargo test --workspace
cargo test -p babble-cli
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p babble-schema --bin export -- bundle ../fixtures/protocol/v1/schema-bundle.json
cargo run -p babble-schema --bin export -- fixtures ../fixtures/protocol/v1/fixtures.json
```

Local API server:

```bash
cd backend
BABBLE_STORE_ROOT=.babble-node \
BABBLE_SEED_PROFILE=card-feed \
BABBLE_PUBLIC_ORIGIN=http://127.0.0.1:8787 \
BABBLE_CORS_ORIGINS=http://127.0.0.1:4321 \
cargo run -p babble-api --bin babble-api
```

`BABBLE_SEED_PROFILE=card-feed` is optional. It creates signed local Objects and content-addressed Web Surface resources only when the store has no Objects.

The API health-checks the Python worker before listening and fails startup if it
is unavailable. Defaults use `algorithms/.venv/bin/python`; deployments can set
`BABBLE_ALGORITHMS_DIR`, `BABBLE_PYTHON_EXECUTABLE`, and
`BABBLE_ALGORITHM_TIMEOUT_MS`. `BABBLE_JUDGMENT_PROVIDER=rust-local` explicitly
selects the older Rust rules; there is no silent fallback. Do not combine that
selection with Python-specific settings. Run one serving node per private store.

## Deterministic Checks

Use the Fozzy determinism engine, whose help banner reads
`deterministic full-stack testing + fuzzing + distributed exploration`.
It is distinct from the `fz` compiler. On this development
machine the engine is `/Users/deepsaint/.cargo/bin/fozzy`; the earlier PATH entry
named `fozzy` currently invokes the compiler. Substitute the engine path below
if needed. Scripted `proc_when` scenarios validate orchestration only; real
product checks require the separate host scenarios. Distributed exploration
requires a distributed scenario and does not apply to these process wrappers.

```bash
fozzy doctor --deep --scenario tests/backend-protocol.fozzy.json --runs 5 --seed 424242 --json
fozzy test tests/backend-protocol.fozzy.json --det --strict --json
fozzy run tests/backend-protocol-host.fozzy.json --det --record artifacts/backend-protocol-host.trace.fozzy --proc-backend host --fs-backend host --http-backend host --json
fozzy trace verify artifacts/backend-protocol-host.trace.fozzy --strict --json
fozzy replay artifacts/backend-protocol-host.trace.fozzy --json
fozzy ci artifacts/backend-protocol-host.trace.fozzy --json
fozzy doctor --deep --scenario tests/sdk-protocol.fozzy.json --runs 5 --seed 626262 --json
fozzy test tests/sdk-protocol.fozzy.json --det --strict --json
fozzy run tests/sdk-protocol.fozzy.json --det --strict --seed 626263 --record artifacts/sdk-protocol.trace.fozzy --json
fozzy trace verify artifacts/sdk-protocol.trace.fozzy --strict --json
fozzy replay artifacts/sdk-protocol.trace.fozzy --json
fozzy ci artifacts/sdk-protocol.trace.fozzy --json
fozzy doctor --deep --scenario tests/developer-cli.fozzy.json --runs 5 --seed 737373 --strict --json
fozzy test tests/developer-cli.fozzy.json --det --strict --json
fozzy run tests/developer-cli.fozzy.json --det --strict --seed 737373 --record artifacts/developer-cli.trace.fozzy --json
fozzy trace verify artifacts/developer-cli.trace.fozzy --strict --json
fozzy replay artifacts/developer-cli.trace.fozzy --json
fozzy ci artifacts/developer-cli.trace.fozzy --strict --json
fozzy doctor --deep --scenario tests/live-stack.fozzy.json --runs 5 --seed 515151 --json
fozzy test tests/live-stack.fozzy.json --det --strict --json
fozzy run tests/live-stack-host.fozzy.json --det --strict --seed 515151 --record artifacts/live-stack.trace.fozzy --proc-backend host --fs-backend host --http-backend host --json
fozzy validate tests/backend-protocol.fozzy.json --json
fozzy fuzz scenario:tests/backend-protocol.fozzy.json --mode property --runs 32 --json
python3 tests/conformance/fixture_conformance.py fixtures/protocol/v1/fixtures.json fixtures/protocol/v1/schema-bundle.json
node tests/live-stack.mjs
```

The live-stack suite owns an isolated Astro process and temporary cache on port
14329, with its own API/store on 18787 and bundle gateway on 18788. It does not
stop the workspace preview on 4321. Production/browser acceptance checks all
three service ports before startup; Astro also rejects its own port conflicts.
Normal completion and assertion failures remove the test services and store.
Astro additionally stops on parent IPC disconnection; abrupt live-stack signals
still need API/store cleanup hardening. Development fixtures may use alternate
`BABBLE_LIVE_*_PORT` values when their own isolation guard permits them;
production and browser-invocation acceptance reserve the fixed ports above.

The browser-invocation acceptance wrapper defaults to an isolated **production
build and preview**. It runs focused consent acceptance and the separate full
social regression against compiled application assets. Auxiliary setup and
component probes are compiled separately under a test-only path in the disposable
output; the application HTML and assets must never reference them. Registration
context is observed read-only in the disposable auth store and checked against
the authenticated API, without patching application classes or transport.

Finish SDK generation/build and stop source edits before setting the freeze
flag. The wrapper hashes product and test sources before and after each run.
Production acceptance reserves API/gateway/frontend ports 18787/18788/14329 and
Aegis 17878. It rejects occupied service ports before starting the fixture.

```bash
BABBLE_FOZZY=/Users/deepsaint/.cargo/bin/fozzy
"$BABBLE_FOZZY" doctor --deep --scenario tests/browser-invocations.fozzy.json --runs 5 --seed 930 --json
"$BABBLE_FOZZY" test --det --strict tests/browser-invocations.fozzy.json --json
export BABBLE_BROWSER_INVOCATION_SOURCE_FROZEN=1
export BABBLE_LIVE_FRONTEND_MODE=production
"$BABBLE_FOZZY" run tests/browser-invocations-host.fozzy.json --det --seed 930 --proc-backend host --fs-backend host --http-backend host --record artifacts/browser-invocations/production-focus.fozzy --record-collision error --json
"$BABBLE_FOZZY" run tests/browser-invocations-full-host.fozzy.json --det --seed 930 --proc-backend host --fs-backend host --http-backend host --record artifacts/browser-invocations/production-full.fozzy --record-collision error --json
for trace in artifacts/browser-invocations/production-focus.fozzy artifacts/browser-invocations/production-full.fozzy; do
  "$BABBLE_FOZZY" trace verify "$trace" --strict --json
  "$BABBLE_FOZZY" replay "$trace" --json
  "$BABBLE_FOZZY" ci "$trace" --json
done
```

Choose new trace filenames for another run; existing evidence is preserved.
`BABBLE_LIVE_FRONTEND_MODE=dev` selects historical development-server behavior.
Other focused fixtures retain development mode until individually migrated.
Native clipboard execution is intentionally excluded, and Aegis fullscreen
rejection is recorded as a limitation rather than native-success evidence.

## Current Scope

Implemented:

- cryptographic identities
- versioned canonical binary encoding for signed/hash-addressed protocol commitments
- deterministic content-addressed IDs over canonical protocol bytes
- core Object schema registry for Babble text/media payloads and capability scope validation
- core realtime schema registry for state/chat payload validation
- Judgment definition registry for spam, relevance, relationship, evidence-quality, content-analysis, and moderation input/output contracts
- Judgment provider orchestration with batch evaluation, cache metadata, provider-declared privacy boundaries, privacy-minimized remote inputs, outage/low-confidence fallback traces, and traceable provider selection
- signed Objects
- signed graph edges
- signed protocol events
- in-memory Object/graph/event state
- provider-independent Judgment contracts and cache
- deterministic local Judgment provider for spam, evidence quality, relevance, relationships, content analysis, and moderation
- Jev-compatible HTTP Judgment adapter isolated behind `JudgmentProvider`
- file-backed identity/Object/edge/event/Judgment/blob storage
- typed Python/uv algorithm workspace for Judgment, discovery, Lens, content analysis,
  moderation, temporal scoring, engagement analytics, consensus, recommendation logic, and
  post-Lens diversity/saturation control
- host capability definitions, permission modes, quotas, scoped grant IDs, grant/revoke decisions, and broker authorization receipts
- Surface runtime admission plans with lifecycle state, resource budgets, sandbox policy, integrity checks, and permission gating
- Surface runtime abuse hardening for unsafe executable entries, ambiguous role declarations,
  target/media mismatches, WebGPU capability declaration, path traversal, and budget escalation
- Surface runtime support for integrity-addressed `babble://blobs/<hash>` executable entries with
  hash validation and declared-resource matching
- WASM Surface execution plans with isolated-store requirements, memory ceilings, fuel and
  epoch-deadline budgets, bounded host-call budgets, denied WASI filesystem/network access, and
  broker-derived capability import descriptors
- deterministic Surface lifecycle scheduler decisions from viewport distance, interaction
  likelihood, memory/GPU pressure, battery saver state, metered network state, and device class,
  including offscreen zero-CPU suspension and critical-pressure eviction budgets exposed through
  host-owned Surface session RPC/HTTP scheduling endpoints, with atomic scheduler application
  that records lifecycle and budget events on the running session
- runtime health snapshots for live Surface sessions, lifecycle counts, aggregate resource
  budgets, granted-capability counts, blocked runtime reasons, and last scheduler/runtime event
  reasons exposed through RPC/HTTP and the TypeScript SDK
- host-owned Surface state checkpoints for suspend/resume flows, with JSON state hashing,
  byte-budget enforcement, durable object-store persistence, runtime audit events, and
  RPC/HTTP restore endpoints
- browser Surface host bridge hardening that binds iframe RPC dispatch to the admitted Object,
  Surface session, expected origin, and backend-issued capability grants
- browser Surface iframe hardening that applies backend CSP restrictions without self-blocking
  Babble embedding, uses credentialless isolated frames when host cookies are unavailable, and
  mirrors lifecycle/resource-budget state for suspension and pressure handling
- browser Surface Permissions Policy hardening that denies ambient device/browser APIs by
  default, denies WebGPU for ordinary Web Surfaces, and enables WebGPU only for admitted
  WebGPU plans with explicit GPU budgets
- browser Surface host pressure handling that applies validated budget reductions and
  deterministically suspends or evicts mounted Surfaces under renderer pressure
- browser Surface resource hardening that explicitly denies worker creation, plugin/object
  loads, base URL rewriting, and form submission at both runtime-plan and served-resource CSP
  boundaries
- browser bridge abuse bounds for inbound request bytes, concurrent in-flight Surface RPCs,
  host dispatch timeouts, and structured rate/quota/timeout errors
- content-addressed media blob descriptors with validated media types, blob URIs, resource conversion, and media Object payload contracts
- validated authoring drafts for text Objects, media Objects, executable Surface declarations, Object resources, graph edges, and capability grant requests
- realtime room/session contracts with membership, persistence, message limits, presence, durable messages, and deterministic collaborative state
- built-in Following, Friends, Research, Intellectual Serendipity, Contradictions, Emerging,
  Slow Internet, and Weird Lens ranking policies
- serializable Lens stacks with weighted composition and ranking traces
- backend post-Lens source diversity that preserves exploration/contradiction floors,
  penalizes single-source collapse, and exposes an inspectable diversity trace on discovery
  responses
- local-only personalization contracts and reranking that consume public Lens candidates plus
  private interests, expertise, muted terms, hidden authors, creator affinities, novelty
  tolerance, exploration preference, and evidence preference without storing or exposing raw
  private terms in protocol/API state
- opt-in encrypted personalization sync contracts for cross-device LocalUserModel transfer,
  using client-held 256-bit sync keys, XChaCha20-Poly1305 authenticated encryption, recipient
  device binding, model metadata as AEAD associated data, tamper/wrong-recipient rejection, and
  schema/SDK-visible ciphertext envelopes without plaintext private model fields
- [device-local feed controls](docs/feed-preferences.md) for interests, expertise,
  word filters, hidden authors with Undo, creator affinity, ranking overrides,
  confirmed history clearing and reset. Following shares the explicit filters
  while retaining chronological order; profiles and conversations remain accessible.
- [account-level block and mute](docs/social-safety.md), separate from device-local
  preferences: private signed state, persistent exact retries, author controls on
  cards/profiles, and a Blocked and muted account view. Blocks reject new local-node
  interactions in either direction without erasing public history; mute only
  filters discovery and Following. Reporting and moderation review remain separate.
- opaque server-side encrypted personalization sync vaulting over file store, node, HTTP, RPC,
  schema, and SDK surfaces, with device-scoped put/list/get/delete operations; list responses
  and observability expose only envelope hashes, identity/device metadata, timestamps, counts,
  and ciphertext byte totals, while fetch returns the encrypted envelope for client-side open
- backend evaluation harness for versioned Judgment calibration fixtures, frozen Lens ranking
  corpora, discovery source-mix coverage, consensus finality, consensus load, realtime room
  state, and realtime load
- provider-matrix Judgment calibration that validates replaceable providers against the same
  definitions, output schemas, expectation corpus, and pairwise agreement checks, including the
  Jev-compatible adapter path through a deterministic transport
- graph-backed mixed candidate generation for evidence, contradictions, semantic neighbors, emerging objects, and exploration
- social graph candidate expansion from signed `follows` edges around anchors and followed Objects, with source contributions preserved in discovery/Lens traces
- backend discovery signals for relevance, novelty, temporal survival, exploration, evidence, contradiction, and multidimensional reputation derived from graph/Judgment/corpus state rather than object-id entropy
- local node API that composes draft authoring, signing, state, store, media publication, graph, publish-time Object Judgment ingestion, discovery, Lens ranking, capability grants, Surface preparation, and realtime room state
- durable node reload/index hydration from file store
- append-only signed event listing, bundle export, and bundle import with cursor pagination,
  parent/target validation, replay deduplication, HTTP routes, typed RPC catalog/schema coverage,
  and SDK helpers
- gossip peer protocol for bounded inventory, event requests, and bundle exchange
- deterministic multi-node gossip simulator with partition, relay, and tamper paths
- deterministic hashgraph event DAG with ancestry, weighted validator quorums, virtual-vote witness fame, finalized ordering, and checkpoint commitments
- signed finality checkpoint publication through node persistence and network gossip
- HTTP/JSON consensus endpoints for checkpoint preview, publication, and lookup
- HTTP/JSON routes for identity, Object, graph edge query, event sync, consensus, and Judgment workflows
- bounded typed graph traversal projections for relation-filtered neighborhoods, evidence/source
  chains, and deterministic graph retrieval through REST/RPC/SDK surfaces
- model-derived relationship inference that evaluates pairwise Judgments, persists the Judgment,
  publishes explicit `JudgmentDerived` graph edges, and carries Judgment provenance in edge metadata
- claim/evidence projections that separate supporting, contradicting, and related evidence while
  preserving human-vs-Judgment origin counts, relationship Judgment provenance, and evidence Object Judgments
- deterministic local Object search API with author, kind, lexical query, scoring, and reason traces; retrieval still scans the store
- HTTP/JSON discovery candidate API over graph, search, Judgment, exploration, and Lens trace signals
- HTTP/JSON, typed RPC, and SDK Lens catalog exposing built-in Lens ids, versions, execution
  modes, required signals, candidate sources, and permission requirements for client inspection
- HTTP/JSON and typed RPC Object Judgment inspection for persisted publish-time spam,
  evidence-quality, content-analysis, and moderation outputs
- HTTP/JSON, typed RPC, and SDK Judgment definition catalog exposing Babble-owned definition
  ids, input/output schema names, semantic meaning, and calibration notes
- HTTP/JSON, typed RPC, and SDK Judgment provider catalog exposing connected provider
  versions, local/remote role, supported definitions, enabled state, and privacy policy
- HTTP/JSON and RPC Judgment evaluation responses with provider orchestration, cache, and privacy decision traces
- HTTP/JSON, typed RPC, and SDK host capability catalog exposing capability ids, versions,
  request/response schemas, permission modes, and quota budgets
- HTTP/JSON capability inspection, grant, revoke, and Surface runtime preparation endpoints
- object-bound `identity.current` capability calls with trusted host identity binding,
  metered broker receipts, RPC catalog/schema coverage, and SDK/Surface bridge helpers
- object-bound `storage.local` capability calls with host-trusted identity partitioning,
  scoped grant namespaces, persistent byte quotas, durable file-backed records,
  RPC catalog/schema coverage, and SDK helpers
- object-bound `social.follow`/`unfollow`/`share`/`reply` capability calls with scoped
  target Object authorization, signed graph edges, signed reply/share text or media Objects,
  broker receipts, RPC catalog/schema coverage, and SDK helpers
- public, paginated `babble.social.quotes.list.v1` / `GET /objects/{id}/quotes`
  for verified source-author quote links; compact shared-post previews and original-post
  navigation preserve feed position, reading position, and profile return history
- object-bound `network.fetch` capability calls with scoped origin grants, request header policy,
  response header filtering, byte quotas, RPC catalog/schema coverage, and SDK helpers
- object-bound `payments.checkout` and `notifications.request` trusted-host capability calls
  with amount/currency/merchant and category/purpose scope enforcement, user-activation action
  plans, broker receipts, RPC catalog/schema coverage, and SDK helpers
- object-bound `media.camera.request` and `media.microphone.request` trusted-host capability
  calls with capture mode, media type, duration, and device-facing scope enforcement,
  user-activation action plans, broker receipts, RPC catalog/schema coverage, and SDK helpers
- object-bound `clipboard.write` and `fullscreen.enter` with durable one-use
  consent, a fresh native-action gesture, typed host-reported outcomes and no
  redispatch on retry; [production-asset consent/protocol acceptance passes](docs/invocation-consent.md#native-browser-methods),
  with physical native completion still unverified
- object-bound `ai.judge` capability calls with scoped Judgment definition/target grants,
  provider-independent orchestration traces, broker receipts, RPC catalog/schema coverage,
  and SDK helpers
- object-bound `ai.generate`/`embed`/`transcribe` trusted-host capability calls with task,
  modality, model, input byte, token, media type, language, and duration scope enforcement,
  provider-secret-free action plans, broker receipts, RPC catalog/schema coverage, and SDK helpers
- object-bound realtime `join`/`send`/`leave` capability calls with room-scope enforcement,
  connection and payload byte accounting, broker receipts on RPC responses, and cross-room
  grant reuse denial
- HTTP/JSON realtime room definition, session start, message commit, and room state endpoints
- HTTP/JSON media blob upload/fetch and signed media Object publication endpoints
- HTTP/JSON, typed RPC, and SDK general Object draft publication for namespaced third-party
  Objects, executable Surfaces, resources, state descriptors, and capability manifests
- versioned typed RPC method catalog for SDK generation, including capability requirements, timeout budgets, and idempotency semantics
- transport-neutral RPC request/response envelopes with runtime/Object binding, deadlines, idempotency keys, trace IDs, and structured SDK/runtime error codes
- RPC host router over the local node with catalog validation, typed payload dispatch,
  required mutation keys, capability-grant authorization, and structured responses;
  ten Object/social publication operations have durable retry receipts committed
  with their signed records (see [the exact scope](backend/crates/store/PUBLICATION.md))
- HTTP/JSON RPC transport endpoint and catalog discovery endpoint for SDK and host clients
- HTTP/JSON, typed RPC, schema, and SDK observability snapshot over aggregate protocol,
  runtime, semantic, discovery, capability, and encrypted personalization sync health without
  raw private personalization data or encrypted envelope bodies
- local `babble-api` server binary with durable store configuration, constrained CORS for browser clients, optional deterministic card-feed seed profile, and content-addressed Surface resource serving
- local `babble` developer CLI for manifest validation/build, local resource hashing, runtime preview/dev-host diagnostics, signed Object inspection, and graph summaries
- generated TypeScript protocol types, RPC method maps, HTTP transport, browser message bridge transport and host listener, browser Surface host mounting, typed client, prepared-Surface SDK binding, capability-aware SDK namespaces, Surface lifecycle controller, and structured error wrapper derived from the checked-in schema bundle
- TypeScript SDK local personalization helpers that rerank public discovery candidates with
  private client-side interests, expertise, hidden authors, muted terms, creator affinities,
  novelty/exploration/evidence preferences, and saturation state while emitting only
  privacy-safe local trace signal names
- TypeScript SDK typed encrypted-personalization envelope metadata helpers aligned with the
  Rust sync contract; actual seal/open remains in the Rust protocol layer until the browser
  runtime ships the same XChaCha20-Poly1305 primitive
- TypeScript SDK `personalization.sync` namespace for encrypted envelope put/list/get/delete
  RPC calls carrying mutation idempotency keys; durable deduplication for this
  mutation family remains unfinished
- Astro frontend shell with real RPC feed loading, search, card-stack navigation, offline/error states, browser Surface preparation, and SDK-backed Surface mounting
- horizontal Astro Object cards that expose protocol identity, resources, Surfaces, capabilities, graph relations, Lens reasons, ranking signals, and inspectable Object manifests
- frontend selector for Balanced, Research, and Weird discovery Lens stacks,
  plus a separate authenticated, chronological Following feed
- frontend authoring composer for authenticated signed text/media Objects,
  idempotent RPC publication, server-backed sessions, and feed refresh
- account-bound HTTP/RPC mutations, independent viewer consent and Surface
  ownership, protected private storage, and separate operator-only operations
- public author profiles with authoritative identity, bounded authored-Object
  pagination, profile-to-swipe-deck navigation, and preserved feed reading position
- private [person-following](docs/following.md), with signed transactional history,
  revision-checked follow/unfollow, durable retry receipts, a private people list,
  and chronological pagination in the rounded swipe deck; distinct from the
  protocol's existing Object-follow capability
- frontend Object Judgment inspector for persisted Judgment loading, definition-specific evaluation, reload, and copy flows
- Astro Surface host integrated with backend-owned runtime sessions, lifecycle transitions,
  scheduler decisions, session IDs, and state checkpoint/restore client hooks
- live-stack scenario that starts the Rust API and Astro frontend, verifies discovery, RPC authoring, content-addressed Surface serving, and browser-visible feed/composer state through Aegis
- Rust-derived JSON Schema bundle for protocol/API contracts with checked-in golden fixtures
- cross-language canonical encoding conformance in Rust, TypeScript SDK, and typed Python algorithms
- stdlib-only fixture conformance verifier that independently checks canonical bytes,
  schema-bundle embedding, registries, RPC catalog/envelope semantics, and media-resource fixtures
- deterministic Fozzy scenarios and recorded traces for backend protocol, developer CLI, and live-stack checks
- typed deterministic Python algorithm modules with a bounded local stdio worker,
  no runtime model downloads, real seven-definition API integration, and a migration
  matrix recording partial semantics and remaining consumers explicitly
- cross-language algorithm source vocabulary aligned around the Rust protocol candidate sources used by discovery and Lens traces

Remaining readiness gaps:

- The [production-readiness ledger](docs/production-readiness.md) tracks the
  authenticated multi-user boundary, complete social and object workflows,
  algorithm integration evidence, and remaining deployment/runtime audits.
- Broaden GPU/device policy evidence within the user's authorized browser tooling.
  Current local checks do not establish production readiness for the full platform.
