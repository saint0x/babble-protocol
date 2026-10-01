#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../../.."
mkdir -p artifacts/indexed-discovery
log="$(mktemp artifacts/indexed-discovery/native.XXXXXX)"
export CARGO_INCREMENTAL=0
if cargo test --manifest-path backend/Cargo.toml -p babel-node -p babel-discovery -p babel-graph -p babel-store -- --test-threads=1 >"$log" 2>&1 &&
   cargo test --manifest-path backend/Cargo.toml -p babel-judgment-python --test provider -- --test-threads=1 >>"$log" 2>&1; then
    printf 'indexed discovery and affected native regressions passed\n'
else
    cat "$log" >&2
    exit 1
fi
