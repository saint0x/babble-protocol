#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p artifacts/publication
log="$(mktemp artifacts/publication/native-tests.XXXXXX)"
if cargo test --manifest-path ../../Cargo.toml -p babel-node --lib judgments::tests -- --test-threads=1 >"$log" 2>&1; then
    printf 'node publication regressions passed\n'
else
    cat "$log" >&2
    exit 1
fi
