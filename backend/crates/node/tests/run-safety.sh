#!/bin/sh
set -eu
cd "$(dirname "$0")/../../../.."
export CARGO_INCREMENTAL=0
mkdir -p backend/crates/node/tests/artifacts/safety
safety_log=$(mktemp "backend/crates/node/tests/artifacts/safety/native.log.XXXXXX")
safety_filter=${1:-safety}
if [ "$safety_filter" = all ]; then safety_filter=""; fi
if cargo test --manifest-path backend/Cargo.toml -p babel-graph -p babel-store -p babel-node -p babel-api -p babel-schema "$safety_filter" -- --test-threads="${2:-1}" >"$safety_log" 2>&1; then
    printf 'safety native graph/store/node/API/schema tests passed\n'
else
    cat "$safety_log"
    exit 1
fi
