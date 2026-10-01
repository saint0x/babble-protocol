#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
log="$(mktemp "${TMPDIR:-/tmp}/babble-social-invocations.XXXXXX")"
trap 'rm -f "$log"' EXIT
mkdir -p artifacts/social-invocations
if CARGO_INCREMENTAL=0 cargo test --manifest-path ../../Cargo.toml -p babble-capabilities -p babble-store -p babble-node -- --test-threads=1 >"$log" 2>&1; then
    cp "$log" artifacts/social-invocations/cargo.log
    printf 'durable social invocation regressions passed\n'
else
    cp "$log" artifacts/social-invocations/cargo.log
    cat "$log" >&2
    exit 1
fi
