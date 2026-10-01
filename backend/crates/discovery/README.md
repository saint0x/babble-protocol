# Discovery

Retrieves a bounded pool of unique public objects for downstream rankers. Graph
relations retain their existing meaning: public object Follows in both directions,
incoming EvidenceFor/EvidenceAgainst, and both directions of References, Cites,
Quotes, Extends, DerivesFrom, Supersedes, Forks, and Remixes.

## Admission policy

- The public request remains `{anchors, followed_objects, limit, exploration_slots}`.
  `limit` is the candidate budget, capped at `MAX_CANDIDATES = 200`; zero returns
  immediately. Empty inputs return no candidates.
- Roots are canonicalized by object ID. At most `MAX_ANCHORS = 64` unique anchors
  and `MAX_FOLLOWED_OBJECTS = 1000` followed objects participate. Excess roots
  are defensively ignored, taking the lexicographically smallest IDs. The caller
  should validate request sizes before retrieval.
- Each source queue contains only IDs with a matching summary. Missing IDs and
  summaries whose embedded object ID differs from their map key are excluded.
  Membership is deduplicated within each source before admission.
- Emerging remains the top ten by the existing weighted temporal/novelty/reputation
  formula. Exploration remains the top `min(exploration_slots, limit, 200)` by
  the existing exploration/novelty/inverse-relevance formula. Score ties use
  newest timestamp, then ascending object ID. Nonfinite values become zero;
  finite unit signals are clamped before scoring.
- Exploration membership reserves that many unique slots first, limited by the
  available valid summaries. An overlapping object consumes one slot and keeps
  its other source memberships. No additional object is labeled Exploration.
- Remaining slots use round-robin admission from eligible queues. Base order is
  Following, Evidence, Contradiction, SemanticNeighborhood, Emerging, Temporal,
  SocialGraph, Exploration, with empty queues removed. The order rotates left by
  `seed % eligible_queue_count`. The seed is fixed FNV-1a over bounded, sorted,
  unique anchor IDs followed by bounded, sorted followed IDs: UTF-8 ID bytes plus
  a zero byte after each ID, and byte 255 after each root group. Offset basis is
  `0xcbf29ce484222325`, multiplier is `0x100000001b3`, arithmetic wraps at 64 bits.
- Each queue visits IDs in ascending order and skips already selected IDs in the
  same turn. Exhausted queues release their turns. If capacity is smaller than
  the eligible classes, exploration reservations win first, then the seeded
  queue order determines which classes get a turn. Representation of every class
  is not promised when capacity or reservations make it impossible.
- Provenance is collected from all queues after selection, even when the pool is
  full. Each membership appears once with weight 1.0. The primary source uses
  the unrotated base priority above. Final output retains the existing ordering:
  primary-source priority, newest timestamp, ascending object ID. Admission does
  not raise relevance, evidence, contradiction, or exploration signals.

Only selected objects become owned Candidates (at most 200). Queue membership
uses borrowed summary keys; graph indexes can require O(N) borrowed IDs. Emerging
and exploration retain only their best k references (k <= 200), avoiding full
summary copies or full candidate-pool allocation. Evidence signals retain their
nonnegative count semantics; other numeric signals are finite and in [0, 1].

## Node integration hook

For ranked discovery, call the existing `CandidateEngine.generate` with
`DiscoveryRequest.limit = MAX_CANDIDATES` (or another candidate budget up to 200).
Apply the user's output limit only after ranking/diversity. Clamp exploration
reservations to the candidate budget and validate root counts with the exported
constants. No new request fields or transport changes are required by this crate.

`followed_objects` and `Relation::Follows` here are legacy public object follows.
Never populate these from the private person-follow graph or private history.
The dedicated chronological following feed is a separate node path and must
continue to bypass discovery/ranking.

## Validation

Run from `backend/crates/discovery`; Cargo uses `CARGO_INCREMENTAL=0` in the
scenario, preserving the existing workspace target cache.

```sh
fozzy doctor --deep --scenario tests/admission-host.fozzy.json --runs 5 --seed 41 --proc-backend host --fs-backend host --http-backend host --json
fozzy test --det --strict tests/admission-host.fozzy.json --proc-backend host --fs-backend host --http-backend host --json
fozzy run tests/admission-host.fozzy.json --det --seed 41 --proc-backend host --fs-backend host --http-backend host --record artifacts/admission-final.fozzy --json
fozzy trace verify artifacts/admission-final.fozzy --strict --json
fozzy replay artifacts/admission-final.fozzy --json
fozzy ci artifacts/admission-final.fozzy --json
CARGO_INCREMENTAL=0 cargo clippy --manifest-path ../../Cargo.toml -p babel-discovery --all-targets --no-deps -- -D warnings
```

The scenario executes the real Rust tests, including graph retrieval, all-source
saturation, complete overlap provenance, exploration reservations, missing IDs,
zero/oversized budgets, canonical root bounds, insertion-order invariance,
small-capacity seeded priority, and nonfinite signal normalization.

On Fozzy 0.1.0 (2685d389c40a), strict host `test` and `run --det` work and
record real subprocess results. Strict `doctor --deep` and scenario `fuzz`
report `proc_unmatched_preflight` / `proc_unmatched` despite host flags.
No scripted success response is substituted for native execution. Replay checks
recorded host observations; it does not rerun Cargo or instrument Rust execution.
Fozzy explore requires a distributed scenario and rejects this process scenario;
the admission algorithm has no distributed schedule to explore. No product
failure trace was produced for shrinking.

Final recorded validation: 14 Rust tests passed; strict trace verification,
replay, CI, rustfmt, and warning-denied Clippy passed. The final host trace is
`artifacts/admission-final.fozzy` (seed 41, run
`be4b6d9a-7316-4c92-8218-072a9455bcaf`). Strict host `test --det` also passed.
Node/API/worker integration and private following feed validation belong to their
respective owners and are outside this crate's test scope.
