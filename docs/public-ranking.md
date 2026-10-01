# Public Ranking

The API's default Python configuration serves public discovery through one
persistent algorithm worker shared with Judgment execution. Native Rust ranking
is an explicit configuration and reference implementation, never an outage fallback.

## Data Flow

1. The node retrieves public Objects from in-memory indexes rebuilt from committed
   records: substring postings, recency, signed inbound relationship activity,
   canonical graph neighbors, explicit roots and deterministic exploration.
   Effective moderation restrictions and explicit search eligibility apply before
   admission slots. No store-wide Object reads or Judgment evaluations occur here.
2. Deterministic round-robin admission selects at most 200 Objects independently
   of output count (except the existing `exploration_slots.min(limit)` policy).
   Graph queues also rotate across roots/relations. Duplicates consume no slots;
   graph/activity membership probes retain contributions outside each queue's
   bounded prefix.
   Requested exploration reserves capacity while leaving one turn for other queues.
3. The node derives public graph, evidence and reputation inputs only for admitted
   Objects. Author counts, maximum author count and population size come from the
   full visible indexed corpus, including for search, rather than the selected pool.
   Versioned [temporal scoring](temporal-scoring.md) runs through the same Python
   worker using one evaluation clock and public relationship activity.
   A non-empty search constrains this pool to at most 200 lexical matches;
   Lens ranking cannot replace an explicit search with unrelated Objects.
   No matches produces an empty result. Blank searches retain mixed discovery.
   Recent Objects and anchors carry `Temporal` provenance. Signed inbound public
   activity carries `SocialGraph` provenance; its retrieval count is an opportunity
   signal, while temporal enrichment validates activity timestamps separately.
   `Emerging` is assigned only to the admitted pool's top 10 by the existing
   0.55 temporal + 0.25 novelty + 0.20 research-reputation formula. `Exploration`
   includes deterministic public sampling and the admitted pool's top requested
   count by the existing 0.50 exploration + 0.35 novelty + 0.15 inverse-relevance
   formula. These sources may overlap; all contributions have weight 1.
   This preserves the formulas, not global full-corpus scoring parity.
4. Python applies the canonical eight Lens rules, weighted composition and
   source-diversity adjustments, returning numeric reasons and full traces.
5. Rust checks the untrusted result against its original request and expected
   provider, then returns Objects in the selected order. Provider failures are
   surfaced as failures, not replaced with native scores.
6. Optional private personalization stays in the browser. Analytics names the
   public model; Object inspection includes its provider/model/version separately
   from local reasons. Empty discovery retains its policy and personalization
   context; the client does not replace it with raw search results. The
   authenticated Following feed remains chronological.

Search retains the existing lexical semantics: ASCII case-insensitive phrase and
any-term substring matches across public Object text/metadata. It is not an
exact-phrase, all-terms, or semantic search promise. Explicit graph anchors,
public Object-follow hints, and exploration cannot broaden its matching pool.

The canonical DTOs live in `backend/crates/lens/src/ranking.rs`. Generated
schemas and SDK types include them and `node.DiscoveryResult.ranking_provider`.
Both API health and discovery expose ranking provenance. Worker configuration,
limits and lifecycle are documented in [algorithm-worker.md](algorithm-worker.md).

## Verification

### Indexed Retrieval

`backend/crates/node/tests/indexed_discovery.rs` exercises the real node with
fixed signed corpus records, 256/1,024 Objects, recording providers, older graph
and activity targets, saturated evidence queues, duplicate imports, restart,
failed imports/publications, moderation restriction/reversal, explicit search
and root limits. Both corpus sizes make exactly 400 uncached Judgment calls and
one 200-item temporal call. This measures provider work, not merely output count.
The generated substring oracle checks Unicode/short substrings, restrictions,
author/kind filters and exact lexical score/order against full-scan expectations.
Admission tests exercise maximum exploration reservations across 64 rotations.

Indexes are rebuilt after store recovery, and updated only after object/edge
persistence succeeds. Duplicate imports do not increase author/activity counts.
Import remains record-by-record; this change does not make an entire import
bundle atomic. Restart rebuilds indexes from durable records. External FileStore
writes are not a live indexing API; use node publication/import or reopen.

The deterministic scenario and real-host runner are
`backend/crates/node/tests/indexed-discovery.fozzy.json` and
`backend/crates/node/tests/run-indexed-discovery.sh`. The runner covers the full
node, discovery, graph and store suites plus the Python provider integration
suite. The final strict host test passed **293 native tests**, including nine
real Python provider integration tests, in run
`29c5da93-5d3b-4978-bbc8-649f25296327`. Its
`artifacts/indexed-discovery/final.fozzy` trace passes strict verification,
replay and CI. `artifacts/indexed-discovery/native.UAUdCD` contains the native
results; `artifacts/indexed-discovery/sources.sha256` verifies the final source
snapshot. These are distinct from scripted doctor/test/fuzz checks. Scenario
fuzzing is not native corpus mutation evidence.
Distributed schedule exploration does not model this synchronous in-process
retriever; native failure/restart tests exercise its actual durable boundaries.

### Final Private Feed

The client now requests up to 200 public candidates when local personalization
is active. `personalizeFeed` applies private filters and preference scoring,
then the canonical source-diversity policy, then the nine-card display limit.
Only selected cards are hydrated. Private preferences never enter that public
request. Without personalization, the public nine-result path is unchanged;
Following remains a separate authenticated chronological feed.

Final local reasons and diversity metadata describe the displayed order, not
the earlier public order. Public scores and provider provenance remain intact
inside the local trace. An entirely filtered pool stays empty; neither search
fallback nor a diversity floor can resurrect excluded content.

Floors and concentration penalties remain soft score adjustments. Membership
uses the primary candidate source, matching the canonical policy; secondary
provenance is retained but cannot count one Object as several floor selections.
These rules do not guarantee a source that is absent from the admitted pool.

Client verification: 19 native fixture comparisons, 42 focused SDK tests and
175 complete SDK tests pass. `sdk/tests/diversity-sdk-protocol-host.trace.fozzy`
passes strict verification, replay and CI. The complete frontend suite passes
805 tests; Astro check and build pass in `artifacts/final-feed-ui-host.fozzy`,
run `2ae73cdd-dc04-4cee-854d-991ad5a529ef`, also verified/replayed/CI.
Focused real API/Astro/Aegis acceptance passes in
`artifacts/feed-diversity-browser/focused-host.fozzy`, run
`e1eab989-d44f-481f-994c-c67e6a95116b`, verified/replayed/CI. The run creates 42
signed posts and four signed graph relationships. Actual Settings controls hide
30 posts and refill nine cards beyond the first public page; it checks all nine
positions, final reasons/metadata, private-field exclusion from serialized
requests, selected-only hydration, three ranked Lenses, empty search and
chronological Following. The observer forwards production methods and transport
unchanged; native fixtures independently establish the diversity formula.
Graph-anchor provenance is a separate API check, not a claim that the UI sends
anchors or uses embeddings. The separate full API/Astro/Aegis regression passes
in `artifacts/feed-diversity-browser/full-host.fozzy`, run
`b2658e94-42b4-46b5-acd3-30153a1f545f` (357.047 seconds), also independently
verified/replayed/CI. It covers existing social, media, account and embedded
Object workflows against the same frozen source. All 438 recorded source and
fixture files remain unchanged. Replay validates recorded behavior; it does not
execute a fresh browser journey.

### Public Worker

`fixtures/protocol/v1/ranking.json` contains 19 native reference cases, including
all eight lenses, blends, zero-weight fallback, ties, nanosecond timestamps,
overlapping provenance and large finite weights. Rust-to-Python tests compare
order, contributions and traces with explicit floating-point tolerance.

Additional tests cover 200 candidates with all eight lenses, malformed and
mutated output, wrong provider, deadline/restart behavior, shared worker lifetime,
configured node invocation, and exclusion of private Following. Fozzy scenarios
are `tests/public-ranking.fozzy.json` (scripted orchestration) and
`tests/public-ranking-host.fozzy.json` (real subprocess execution).

Verified host traces are `artifacts/public-ranking-final-host.trace.fozzy` and
`artifacts/live-stack-public-ranking-verified-host.trace.fozzy`; both pass strict
verification, replay and CI. The live run checks health/discovery/inspection
provenance as well as real social, image, Following and Surface flows. New public
publications omitted from a ranked page are directly opened in the deck with no
invented rank; private Following remains untouched.

## Remaining Limits

- Provider enrichment is bounded to 200 Objects, two Judgment lookups per Object,
  and one temporal batch. Lexical scoring still visits matching posting entries
  to preserve exact score/order; common substring queries may match the corpus.
  Filtered graph/recency/activity iterators may skip restricted or duplicate IDs;
  enrichment costs also depend on admitted Objects' graph degrees. This is not a
  constant-time guarantee for arbitrary search or graph degree.
- Recency, public activity and deterministic sampling bound the retrieval scope;
  Emerging/exploration scoring cannot identify a globally optimal Object outside
  that pool. Activity counts do not establish unique readers or trusted engagement.
- Diversity floors/shares are soft score adjustments, not hard guarantees.
  Private ordering now reapplies them, but the admitted pool limits coverage.
- Graph semantic-neighbor relations are not an embedding model. The seven lexical
  Judgments are not calibrated semantic intelligence.
- Semantic source-agreement quality, engagement consumers, temporal-policy evaluation,
  and deployment/load readiness remain open in [the migration audit](algorithm-migration.md)
  and [production readiness](production-readiness.md).
