#!/usr/bin/env bash
set -euo pipefail
export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS=2
export RUST_TEST_THREADS=1
log=$(mktemp)
trap 'rm -f "$log"' EXIT
if ! cargo test --manifest-path ../../Cargo.toml -p babel-api -p babel-node -p babel-store --lib invocation -- --test-threads=1 >"$log" 2>&1; then
    cat "$log" >&2
    exit 1
fi
sed -n '/^test result:/p' "$log" >&2
if ! cargo test --manifest-path ../../Cargo.toml -p babel-capabilities -p babel-runtime -p babel-rpc --lib -- --test-threads=1 >"$log" 2>&1; then
    cat "$log" >&2
    exit 1
fi
sed -n '/^test result:/p' "$log" >&2
printf 'browser focused consent and store regressions passed\n'
