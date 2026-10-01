# Local Algorithm Worker

Implementation contract for the Rust/Python execution boundary. The protocol core
owns Judgment records, IDs, hashes, timestamps, validation and persistence. Python
returns algorithm output only. This worker is local, with no network listener.

## Wire Contract

UTF-8 newline-delimited JSON on stdin/stdout. Exactly one response per request.
No logs on stdout. Ranking and temporal frames are at most 4 MiB including newline; Judgment
frames retain their 1 MiB limit. JSON numbers
must be finite; duplicate keys are invalid. IDs are positive integers no larger
than 9007199254740991. Invalid input returns a structured error, never a score.

Health request:

```json
{"protocol":"babel.algorithms.v1","id":1,"method":"health"}
```

Judgment request:

```json
{"protocol":"babel.algorithms.v1","id":2,"method":"judge","request":{"definition":"babel.judgment.spam.v1","state":{"subject":"obj_example","context":{"text":"Example content"}},"parameters":{}}}
```

Response envelope always has `protocol`, `id`, `result`, and `error`. Exactly one
of result/error is non-null. Unparseable request IDs yield a null response ID.
Health result is `{provider, ranking_provider, temporal_provider, supported_definitions}`. Judge result is
`{provider, output, confidence}`; output follows the existing Rust registry's
definition-specific schema. Provider is
`{provider:"babel-python",model:"lexical-v1",version:"1"}`. These are lexical
algorithms, not trained semantic models or calibrated probabilities.

Ranking uses `method: "rank"` with the canonical `lens.RankingRequest`:
`{candidates, lens, diversity, limit}`. Its result is
`{ranked, trace, diversity_trace, provider}`, with provider
`{provider:"babel-python",model:"lenses-v1",version:"1"}`. All eight public
lenses, blends, numeric reasons and soft source-diversity adjustments run in
typed Python. Rust validates candidate identity, provenance, finite scores,
trace consistency, lengths and order without rerunning the scoring model.
Cross-language golden fixtures establish parity separately.

Temporal scoring uses `method: "temporal"` with
`discovery.TemporalRequest`: `{reference_time, items}`. It returns
`{provider, reference_time, scores}` under `babel-python/temporal-v1/1`.
The full typed scorer runs for at most 200 unique Objects per frame; the node
feeds its output into public discovery and returns selected scores for inspection.
See [temporal scoring](temporal-scoring.md) for time, activity, and privacy rules.

Requests admit at most 200 unique candidates, eight unique weighted lenses,
eight unique sources per candidate, and eight source floors. Output limit is
1..200. Empty/all-zero stacks use Following's public Lens rule, not the private
chronological person-following feed. General frames allow 200,000 JSON values;
Judgment request trees retain 4,096. Private follow lists and reading histories
never enter this worker. See [public ranking](public-ranking.md).

Errors are `{code, message}`, with code `invalid_request`,
`unsupported_definition`, or `algorithm_failure`. Error text must not echo
subject text, private context, or a traceback.

Supported definitions are the seven existing core definitions: spam,
evidence_quality, relevance, relationship, content_analysis, moderation and source_agreement (each
under `babel.judgment.*.v1`). Existing library algorithms must be called, not
reimplemented in a transport handler. Sentiment maps [-1,1] to [0,1]; Python
safety risk maps to the core's safety score as `1 - risk`. The advisory `warn`
action maps to `flag`, with its original advisory action retained in output.
Misinformation markers remain lexical signals, never fact-checking claims.

The request is the core `JudgmentRequest`. Text is required, nonblank, at most
128 KiB UTF-8. Subject is nonblank, at most 4096 bytes. Context/parameter maps
have at most 64 entries each, JSON nesting at most 16. Unsupported parameter
types fail explicitly. Arrays have at most 256 elements; JSON trees have at most
4096 values. Object keys are limited to 4096 UTF-8 bytes.
Relationship requests preserve the requested relation;
they cannot relabel a contradictory finding as support. Parameters/query/context
must remain bound in the Rust cache and commitment.

Rust exports the versioned DTO schemas and real-worker conformance fixtures into
`fixtures/algorithms/v1/`. Both languages test them. Runtime validation also
enforces cross-field conditions such as matching envelope/output confidence.

Content/moderation have no calibrated confidence estimator: they return `0.0`
with `confidence_status: "uncalibrated"`, not a claimed zero probability. The
other four definitions expose `confidence_status: "legacy_heuristic"`.
The frontend labels these states instead of displaying confidence percentages.
Relationship output explicitly reports its marker scope and that it does not
evaluate target entailment. A combined source/target text input is not pairwise
semantic evidence, even when it contains a support or contradiction marker.
The local provider now preserves explicit public `source_text` and `target_text`
through orchestration. Source-only markers cannot be contaminated by words in
the target; both texts bind the input commitment. This does not change the remote
privacy policy or turn the local relationship algorithm into entailment inference.

## Configuration

Run `uv sync --frozen --project algorithms` from the repository root before
starting the API. `python -I -m babel_algorithms.worker` uses the installed package,
not a caller-controlled `PYTHONPATH`.

| Setting | Default / Meaning |
| --- | --- |
| `BABEL_JUDGMENT_PROVIDER` | `python` for Judgment, public ranking, and temporal scoring; `rust-local` explicitly selects their native implementations |
| `BABEL_ALGORITHMS_DIR` | Repository `algorithms/`; relative overrides resolve against the server's working directory |
| `BABEL_PYTHON_EXECUTABLE` | `.venv/bin/python` in the algorithm directory; prefer an absolute path |
| `BABEL_ALGORITHM_TIMEOUT_MS` | `5000`; allowed range 1 through 120000 milliseconds |

Python-specific settings with `rust-local` are rejected, not ignored. Relocated
deployments must supply their installed paths rather than relying on a compile-time
repository location. The API's health response identifies the selected provider;
startup performs an actual Python health exchange before binding the listener.

## Lifecycle

The Rust adapter starts a configured executable directly, with separate arguments
and no shell. Startup performs a real health exchange. One call deadline covers
worker contention, restart, writes, and reads. Timeout, malformed response, wrong
ID/provider, oversized output, and process exit invalidate the worker. Never
silently return Rust-rule output under Python provenance. A subsequent request
may start a fresh worker; successful Judgments alone enter the cache.

The worker receives a minimal environment without inherited operator secrets.
Unix processes have a dedicated process group, 64 descriptors, no core dumps,
and no regular-file growth. Linux adds a 512 MiB address-space limit. macOS
samples resident memory during active calls and invalidates above 512 MiB; this
can overshoot and does not monitor idle workers or descendants. Linux behavior
has not been verified on this macOS host. Group termination/reaping depends on
kernel progress. These bounds are not a hostile-code sandbox.

The API admits at most 16 concurrent handler jobs, buffers bodies asynchronously
with a 16 MiB plus 64 KiB envelope cap and 10-second body deadline, and runs
synchronous node work off
the async reactor. Saturation returns 503 with Retry-After. A disconnected caller
does not release admission while its mutation still runs. The node remains
serialized; this is bounded execution, not a demonstrated throughput target.

Publication prepares and validates all four ingestion Judgments before writing
the Object, event, index, or Judgment cache. Provider failures therefore do not
leave an apparently failed new post committed. The current publication path then
uses the store's durable batch and redo-journal recovery before live index/cache
updates; the original preflight-only limitation has been superseded.

The configured process is trusted local deployment code, not a Surface sandbox.
Do not pass private user/peer histories as public discovery input. Source agreement
now has a signed-public-evidence consumer; private recommendation and engagement
still require complete live consumers recorded in the migration matrix.

## Integration Evidence

The original API/Astro/Aegis worker milestone invoked Python for six definitions, checked
publish-time Judgments and repeated evaluations, and reads provider/confidence
labels in the browser. It also runs the authenticated account, follow, reply,
share, image, nested-thread, and executable-Surface flows. The recorded trace
`artifacts/live-stack-python-verified-host.trace.fozzy` passed strict verification,
replay, and CI on 2026-09-29 local time. Scripted strict doctor/test ran first;
those scripted checks alone are not evidence of browser or Python execution.

The provider's real-worker/fault tests and the Python worker's input/conformance
tests have separate host-backed traces documented alongside their scenarios.
Transport correctness does not establish semantic accuracy or whole-platform
production readiness.

The subsequent public-pair scope regression and full provider suite pass in
`artifacts/relationship-pair-provider-host.fozzy`, run
`401ed575-bad8-4614-aee1-98e84e1fdf05`, strictly verified, replayed and passing CI.
It exercises the real installed Python worker through the Rust orchestrator,
checks both input commitments and confirms private-history exclusion. Current
real-worker conformance covers all seven definitions.
