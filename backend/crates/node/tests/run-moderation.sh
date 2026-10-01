#!/bin/sh
set -eu
cd "$(dirname "$0")/../../../.."
export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS=2
mkdir -p backend/artifacts/moderation
moderation_log=$(mktemp "backend/artifacts/moderation/native.XXXXXX")
if (
    cargo test --manifest-path backend/Cargo.toml -p babel-node -p babel-api --test moderation -- --test-threads=4 &&
    cargo test --manifest-path backend/Cargo.toml -p babel-node --test moderation_projections -- --test-threads=3 &&
    cargo test --manifest-path backend/Cargo.toml -p babel-api --lib moderation -- --test-threads=4 &&
    cargo test --manifest-path backend/Cargo.toml -p babel-schema --lib -- --test-threads=4
) >"$moderation_log" 2>&1; then
    printf 'moderation native graph/store/node/API/schema tests passed\n'
else
    cat "$moderation_log"
    exit 1
fi
