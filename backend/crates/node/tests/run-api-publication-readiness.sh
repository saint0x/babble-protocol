#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p artifacts/atomic-publication
log="$(mktemp artifacts/atomic-publication/api-readiness-tests.XXXXXX)"
if CARGO_INCREMENTAL=0 cargo test --manifest-path ../../Cargo.toml -p babel-api --lib publication_recovery_required -- --test-threads=1 >"$log" 2>&1; then
    printf 'API publication readiness regression passed\n'
else
    cat "$log" >&2
    exit 1
fi
