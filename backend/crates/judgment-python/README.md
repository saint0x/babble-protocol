# Python Algorithm Provider

`PythonProvider::new(WorkerConfig)` starts and health-checks a persistent local
worker. `WorkerConfig` has `executable`, `args`, `working_directory`, and `timeout`.
Use an installed interpreter with `-I -m babble_algorithms.worker`. The provider
is synchronous and Send + Sync; async callers must provide a bounded blocking
execution boundary. Timeout must be positive and at most 300 seconds.

The crate separates versioned DTOs/schema export (`contract`), direct subprocess
and pipe lifecycle (`transport`), core Judgment construction (`lib`), and
canonical public ranking (`ranking`). The same process implements both provider
traits. Ranking keeps native candidate identities and validates numeric traces;
private Following and browser histories are excluded. Rust
validates output, binds parameters to commitments, and owns hashes, IDs and time.
No algorithm fallback or cache lives here. Invalid input returns Conflict;
transport, protocol and algorithm failures return sanitized ProviderUnavailable.

Each call has one deadline covering lock contention, restart health, writes and
reads. Pipes use nonblocking I/O with bounded frames; no reader threads exist.
Any failed exchange invalidates, kills and reaps the worker. Drop kills its
dedicated process group and reaps the direct child. This trusts the configured
local executable: it is not a sandbox against a process escaping its group.
Process creation and SIGKILL reaping still depend on the OS kernel making progress.

The child receives only PATH=/usr/bin:/bin and LANG=C.UTF-8 (Python may add
LC_CTYPE and macOS may add __CF_USER_TEXT_ENCODING). It does not inherit operator
credentials or Python environment hooks.
Use an absolute executable for virtual environments. stderr is discarded.

Unix resource bounds: 64 descriptors, no core dumps or regular-file growth;
Linux additionally enforces 512 MiB address space. macOS rejects AS/DATA limits
on the installed runtime, so active exchanges sample resident memory at most
every 20 ms and invalidate above 512 MiB. That sampled limit can overshoot and
does not monitor an idle worker or descendants. There is no cumulative CPU
limit: active computation is bounded by the per-call deadline. Linux resource
behavior requires verification on a Linux host; current integration ran on macOS.

From `backend`, run `cargo test -p babble-judgment-python`. Real-worker tests are
required and fail if the installed worker is missing. Set BABBLE_TEST_PYTHON to
override `algorithms/.venv/bin/python`. `cargo run -p babble-judgment-python
--example export` regenerates the checked Rust schemas and real-worker fixtures
under `fixtures/algorithms/v1`. Registry validation remains authoritative for
definition-specific outputs and semantic conditions such as confidence equality.

Public ranking scenarios are at repository-root `tests/public-ranking.fozzy.json`
and `tests/public-ranking-host.fozzy.json`. Run strict scripted doctor/test first,
then record the host scenario with host process/filesystem/HTTP backends for
actual node, API and worker evidence. See `docs/public-ranking.md` for scope.
Scripted process expectations alone do not execute Rust or Python.
