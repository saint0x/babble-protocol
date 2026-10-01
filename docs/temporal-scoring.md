# Temporal Scoring

Public discovery invokes the typed Python `TemporalScorer` through the shared
local algorithm worker. Its version is `babel-python/temporal-v1/1`. The explicit
native configuration uses `babel-rust/temporal-v1/1`; a Python failure does not
silently select it.

## Inputs And Meaning

Each discovery request captures one real evaluation time. Quiet feeds continue
to age even when nobody publishes. Native replay can supply an explicit time
through `LocalNode::discover_objects_at`.

The node supplies public Object publication times, a lexical time class, quality
signals, temporal tags, and counts of signed inbound content relationships.
Whole-word rules distinguish news, discussion, analysis, tutorials, and reference
material. They are transparent English heuristics, not inferred semantic truth.

Public activity counts include human/application assertions of replies, quotes,
citations, evidence, support, contradiction, references, and creative lineage.
They exclude duplicate edge IDs, self-edges, unsigned/derived relationships,
non-content relationships, dates before the Object, and future dates. Recent
means the inclusive seven days preceding evaluation. Counts describe edges,
not unique people, independent sources, satisfaction, or verified agreement.
Views are zero because the platform does not collect public view telemetry.
Private follow lists, reading duration, scroll depth, and peer histories are
not sent to this scorer. This is not an engagement-analytics ingestion service.

The full scorer computes age, class-weighted recency, tag-adjusted time
sensitivity, recent/total activity fractions with age decay, a bounded decay
rate, and survival score. The survival score feeds public Candidate temporal
and exploration signals before Lens ranking. It is a heuristic, not a
probability, truth score, or the platform's sole ranking objective. Future
publication times explicitly have zero age and zero activity velocity.

## Boundary

Canonical DTOs are in `backend/crates/discovery/src/temporal.rs`, exported as
`discovery.TemporalRequest`, `discovery.TemporalResult`, and
`discovery.TemporalProviderVersion`. Worker method `temporal` accepts
`{reference_time, items}`. Each item has Object ID, publication time, time class,
quality, tags, and engagement counts. Result scores retain input order and
identity. Response provenance and reference time must match the request.

Frames contain at most 200 unique Objects. Numeric scores must be finite and in
range; counts must be safe nonnegative integers with recent counts no larger
than totals. Unknown fields and malformed timestamps fail explicitly. Integer
nanosecond timestamps are subtracted before conversion to floating-point age.
The shared worker enforces deadlines, output bounds, fault invalidation, and
restart. There is no per-Object subprocess and no unversioned score cache.

`DiscoveryResult.temporal` contains the scores for returned Objects in ranked
order. Analytics shows compact values; the inspector preserves all components,
exact evaluation time, and provider/model/version. Direct publications, profiles,
and chronological Following do not invent temporal evaluations.

## Verification And Limits

`fixtures/protocol/v1/fixtures.json` contains `temporal_scoring` reference cases
for all classes, age boundaries (including two hours plus one nanosecond), tags,
activity without views, and supported timestamp extremes. Unit, real-worker,
node/API, and live Aegis tests cover different boundaries; passing native parity
does not establish that the heuristic is empirically calibrated.

The deprecated formulas are not claimed numerically identical: the typed
replacement has explicit five-class weights, inclusive recency boundaries,
bounded decay, and a survival output. The node previously used a separate graph
degree proxy and the newest publication as its clock; those paths are removed.

Discovery now uses [indexed retrieval](public-ranking.md) before enriching at
most 200 candidates in one temporal batch. Recency and public inbound activity
provide admission opportunities; temporal enrichment still validates activity
timestamps. Author statistics retain full visible-corpus scope. Lexical search
visits matching postings, and high-degree graph enrichment remains data-dependent;
bounded provider work is not a constant-time query or load-readiness guarantee.
The source-agreement consumer has separate verification. Semantic classification,
empirical temporal-policy evaluation, private recommendation adaptation and opt-in
engagement analytics remain part of the broader algorithm migration.

The current verification includes 485 Python tests, strict typing and lint,
real Python/native parity, nine node temporal integration tests, three temporal
API tests, and four search API regressions. The full Rust workspace passes.
All 248 frontend tests, 65 SDK tests, independent fixture conformance, and Astro
checking/build also pass. Astro retains two existing async-conversion hints.

Recorded host evidence (strictly verified, replayed, and CI-checked):

- `artifacts/temporal-search-verified-host.trace.fozzy`
- `artifacts/discovery-search-api-host.trace.fozzy`
- `artifacts/temporal-search-client-host.trace.fozzy`
- `algorithms/artifacts/temporal-adversarial-host.fozzy`
- `artifacts/rounded-cards-reading-restored-host.trace.fozzy`

These traces have distinct scopes: node/API integration, search API behavior,
client regressions, adversarial Python contracts, and the actual full browser
workflow. They are not deployment
or physical-device certification.
