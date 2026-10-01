"""Stable Fozzy host observation of the real ranking and worker regression tests."""

import subprocess
import sys
from pathlib import Path


def main() -> int:
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "tests/test_ranking.py", "tests/test_worker.py"],
        cwd=Path(__file__).parents[1],
        capture_output=True,
        timeout=120,
    )
    if result.returncode:
        _ = sys.stdout.buffer.write(result.stdout)
        _ = sys.stderr.buffer.write(result.stderr)
        return result.returncode
    print("ranking subprocess checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
