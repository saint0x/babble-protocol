#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p artifacts/atomic-publication
log="$(mktemp artifacts/atomic-publication/native-tests.XXXXXX)"
packages=(-p babble-store)
if [[ "${1:-all}" == "all" ]]; then
    packages+=(-p babble-node)
fi
if /Users/deepsaint/.cargo/bin/cargo test --manifest-path ../../Cargo.toml "${packages[@]}" -- --test-threads=1 >"$log" 2>&1; then
    printf 'atomic publication regressions passed\n'
else
    cat "$log" >&2
    exit 1
fi
