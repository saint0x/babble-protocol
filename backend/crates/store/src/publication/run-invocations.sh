#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."
log="$(mktemp "${TMPDIR:-/tmp}/babble-invocations.XXXXXX")"
trap 'rm -f "$log"' EXIT
if CARGO_INCREMENTAL=0 cargo test --manifest-path ../../Cargo.toml -p babble-capabilities -p babble-store --lib -- --test-threads=1 >"$log" 2>&1; then
    printf 'invocation domain and store regressions passed\n'
else
    cat "$log" >&2
    exit 1
fi
