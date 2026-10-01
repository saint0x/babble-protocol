#!/bin/sh
set -eu
cd "$(dirname "$0")/../../../.."
export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS=2
mkdir -p backend/artifacts/moderation
moderation_log=$(mktemp "backend/artifacts/moderation/regression.XXXXXX")
if (
    cargo test --manifest-path backend/Cargo.toml -p babble-api --test auth --test security --test following --test capability_consent -- --test-threads=4 &&
    cargo test --manifest-path backend/Cargo.toml -p babble-node --test following --test safety --test session_permissions -- --test-threads=1 &&
    cargo test --manifest-path backend/Cargo.toml -p babble-api --lib gateway::tests -- --test-threads=4
) >"$moderation_log" 2>&1; then
    printf 'moderation adjacent auth runtime social regressions passed\n'
else
    cat "$moderation_log"
    exit 1
fi
