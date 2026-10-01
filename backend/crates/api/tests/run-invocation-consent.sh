#!/usr/bin/env bash
set -euo pipefail
export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS=1
log=$(mktemp)
trap 'rm -f "$log"' EXIT
if ! cargo test --manifest-path ../../Cargo.toml -p babel-api -p babel-runtime -p babel-rpc --tests --no-fail-fast >"$log" 2>&1; then
    cat "$log" >&2
    exit 1
fi
sed -n '/^test result:/p' "$log" >&2
printf 'invocation consent API and runtime regressions passed\n'
