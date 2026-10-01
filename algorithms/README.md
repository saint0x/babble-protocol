# Babel Algorithms

This package contains typed, deterministic Python primitives for content analysis, lexical
Judgment fallback, candidate assembly, Lens ranking, feed diversity, recommendation,
moderation, source agreement, temporal scoring, and engagement analytics.

The served Rust API invokes this package through one persistent local worker for
all seven Judgment definitions, including on-demand
[source agreement](../docs/source-agreement.md), canonical public discovery ranking, and
[temporal scoring](../docs/temporal-scoring.md). The
TypeScript client reaches it through the API, not by executing Python in the
browser. Ranking implements the eight protocol Lenses, weighted blends and soft
source-diversity adjustments with complete traces. Its provider is
`babel-python/lenses-v1/1`; the lexical Judgment provider remains
`babel-python/lexical-v1/1`. Neither is a trained semantic model or calibrated
probability estimate.

See [public ranking](../docs/public-ranking.md) for the end-to-end data flow,
provenance, parity coverage and remaining limits, and
[the migration matrix](../docs/algorithm-migration.md) for other algorithm
families and their remaining integration work. Simulation and model evaluation
harnesses currently live in Rust, not in this package.

The contract is intentionally narrow:

- Python is allowed for production algorithm development, research, and service-side ranking.
- Protocol records, signatures, canonical IDs, and durable schemas still belong to the core backend.
- Algorithm output should remain explainable, deterministic under fixed inputs, and portable back into protocol-level `Judgment`, `Candidate`, and `RankingTrace` shapes.

## Modules

### Canonical Worker Path

- `ranking_types`: typed dataclasses implementing the public ranking request,
  result, provenance and trace DTOs. Rust owns the authoritative schemas.
- `ranking_wire`: strict public input validation, including unknown fields,
  numeric bounds, unique IDs, Lens weights and source contributions.
- `ranking`: the eight protocol Lens formulas, weighted composition and ranking
  traces, matching the native Rust reference.
- `ranking_time`: RFC3339 canonicalization and integer nanosecond tie-breaking.
- `ranking_diversity`: source-only floor bonuses and concentration penalties.
  Floors and source shares are soft adjustments, not hard quotas.
- `judgment`: deterministic lexical Judgment provider.
- `worker`, `wire`, `execution`: persistent `babel.algorithms.v1` NDJSON transport,
  strict runtime validation, health/provenance, `judge` execution and `rank`
  execution, plus batched `temporal` scoring. Rust retains record creation, signatures, commitments, cache and
  persistence.
- `canonical`: cross-language canonical encoding for Python-side conformance checks.

Canonical ranking accepts only public candidate metadata and supplied numeric
signals. It rejects non-finite or out-of-range values instead of silently
normalizing them. Empty/all-zero Lens stacks fall back to Following; overflowing
weight sums use stable max-scaled normalization. Results include all input
candidates in the ranking trace and the selected prefix in the diversity trace.

### Analysis And Legacy Utilities

- `content`: deterministic text properties, topics, evidence, sentiment, and summary analysis.
- `moderation`: community moderation signals, spam/quality/safety scoring, and action policy.
- `temporal`: recency, decay, time sensitivity, engagement velocity, and survival scoring.
- `engagement`: session aggregation, trend buckets, user segments, and content performance.
- `consensus`: source reliability, term/fact agreement, consensus states, and user contributions.
- `recommendation`: content/user vectors, collaborative scoring, temporal blending, and Candidate output.
- `RecommendationWeights.with_feedback`: immutable, bounded adaptation from explicit ratings;
  pass the returned weights into `RecommendationEngine`. Callers own per-user storage and
  batch deduplication. Only interactions at or before `reference_time` enter recommendations.
- `discovery`: legacy candidate assembly; explicit signals take precedence over
  source-specific defaults. Public candidate admission is owned by Rust.
- `lens`: legacy recommendation-family ranking using recency, weirdness and
  emerging signals. Its coefficients and models differ from public Lens semantics.
- `diversity`: legacy personalized feed reranking using creator concentration,
  topic saturation, seen-object repetition and soft source-floor bonuses.

The legacy recommendation, Lens and diversity utilities are not the public
worker's ranking path. Their private personalization fields are not accepted by
`RankingRequest`. Legacy Lens inputs are normalized before weighting; invalid
weights raise `ValueError` and non-finite unit scores normalize to zero. These
local dataclasses are distinct from the canonical wire DTOs in `ranking_types`.

## Run

```bash
uv sync --frozen
uv run --frozen pytest
uv run --frozen basedpyright
uv run --frozen ruff check
```

Run these commands from `algorithms/` so the tools load this package's configured
test/type/lint scope. The installed worker starts with
`.venv/bin/python -I -m babel_algorithms.worker`; stdout is exclusively protocol
NDJSON. See [the worker contract](../docs/algorithm-worker.md) for limits,
configuration, confidence semantics, and failure behavior.
