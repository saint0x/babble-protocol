#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."
log="$(mktemp "${TMPDIR:-/tmp}/babble-store-publication.XXXXXX")"
if CARGO_INCREMENTAL=0 /Users/deepsaint/.cargo/bin/cargo test --manifest-path ../../Cargo.toml -p babble-store --lib publication:: -- --test-threads=1 >"$log" 2>&1; then
    printf 'store publication regressions passed\n'
else
    cat "$log" >&2
    exit 1
fi
