#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p artifacts/object-judgments
log="$(mktemp artifacts/object-judgments/store-tests.XXXXXX)"
if CARGO_INCREMENTAL=0 cargo test --manifest-path ../../Cargo.toml -p babel-store -- --test-threads=1 >"$log" 2>&1; then
    printf 'object judgment persistence and store regressions passed\n'
else
    cat "$log" >&2
    exit 1
fi
