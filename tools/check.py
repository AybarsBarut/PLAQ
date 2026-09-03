#!/usr/bin/env python3
"""Run PLAQ's complete quality gate locally, without hosted CI."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> None:
    print(f"\n> {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=ROOT, check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--quick",
        action="store_true",
        help="skip documentation and fuzz-harness compilation",
    )
    args = parser.parse_args()

    commands = [
        ["cargo", "fmt", "--all", "--", "--check"],
        [
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        ["cargo", "test", "--workspace", "--all-features"],
        [
            sys.executable,
            "-m",
            "py_compile",
            "tools/analyze.py",
            "tools/check.py",
            "tools/generate_synthetic.py",
            "tools/visualize_groove.py",
        ],
    ]
    if not args.quick:
        commands.extend(
            [
                ["cargo", "doc", "--workspace", "--no-deps"],
                ["cargo", "check", "--manifest-path", "fuzz/Cargo.toml"],
            ]
        )

    try:
        for command in commands:
            run(command)
    except FileNotFoundError as error:
        print(f"quality gate could not start: {error}", file=sys.stderr)
        return 127
    except subprocess.CalledProcessError as error:
        print(
            f"quality gate failed with exit code {error.returncode}",
            file=sys.stderr,
        )
        return error.returncode

    print("\nPLAQ local quality gate passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
