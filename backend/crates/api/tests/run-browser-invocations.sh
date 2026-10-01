#!/usr/bin/env bash
set -euo pipefail
export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS=2
export RUST_TEST_THREADS=1
log=$(mktemp)
trap 'rm -f "$log"' EXIT
if ! cargo test --manifest-path ../../Cargo.toml -p babble-capabilities -p babble-store -p babble-node -p babble-api -p babble-runtime -p babble-rpc -p babble-schema --tests --no-fail-fast -- --test-threads=1 >"$log" 2>&1; then
    cat "$log" >&2
    exit 1
fi
sed -n '/^test result:/p' "$log" >&2
printf 'browser invocation and affected backend regressions passed\n'
