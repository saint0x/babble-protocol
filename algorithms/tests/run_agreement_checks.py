"""Stable host-backed Fozzy observation of the real source agreement integration."""

import subprocess
import sys
from pathlib import Path


def main() -> int:
    root = Path(__file__).parents[2]
    checks = (
        ([sys.executable, "-m", "pytest", "-q"], root / "algorithms"),
        (["uv", "run", "--frozen", "basedpyright"], root / "algorithms"),
        (["uv", "run", "--frozen", "ruff", "check", "."], root / "algorithms"),
        (["/usr/bin/env", "CARGO_INCREMENTAL=0", "cargo", "test", "--quiet",
          "-p", "babble-judgment-python", "-p", "babble-judgment-local",
          "-p", "babble-judgment", "-p", "babble-schema"], root / "backend"),
        (["node", "scripts/generate-protocol.mjs", "--check"], root / "sdk"),
    )
    for command, cwd in checks:
        result = subprocess.run(command, cwd=cwd, capture_output=True, timeout=240)
        if result.returncode:
            _ = sys.stdout.buffer.write(result.stdout)
            _ = sys.stderr.buffer.write(result.stderr)
            return result.returncode
    print("source agreement Python, Rust, schema, and generated SDK checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
