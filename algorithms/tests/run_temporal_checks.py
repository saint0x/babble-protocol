"""Stable Fozzy observation of the complete Python test, type, and lint checks."""

import subprocess
import sys
from pathlib import Path


def main() -> int:
    exit_code = 0
    for command in (
        [sys.executable, "-m", "pytest", "-q"],
        ["uv", "run", "--frozen", "basedpyright"],
        ["uv", "run", "--frozen", "ruff", "check", "."],
    ):
        result = subprocess.run(
            command, cwd=Path(__file__).parents[1], capture_output=True, timeout=180
        )
        if result.returncode:
            _ = sys.stdout.buffer.write(result.stdout)
            _ = sys.stderr.buffer.write(result.stderr)
            exit_code = result.returncode
    if exit_code:
        return exit_code
    print("temporal full Python pytest, basedpyright, and ruff checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
