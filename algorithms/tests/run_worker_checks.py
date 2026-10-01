"""Give Fozzy a stable host-process observation while running real subprocess tests."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def main() -> int:
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "tests/test_worker.py"],
        cwd=Path(__file__).parents[1],
        capture_output=True,
        timeout=120,
    )
    if result.returncode:
        sys.stdout.buffer.write(result.stdout)
        sys.stderr.buffer.write(result.stderr)
        return result.returncode
    print("worker subprocess checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
