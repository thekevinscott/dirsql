"""Console-script entry point. Execs the bundled binary on POSIX,
subprocesses it on Windows."""

from __future__ import annotations

import os
import subprocess
import sys

from dirsql._cli.binary_path import binary_path
from dirsql._cli.is_windows import is_windows


def main(argv: list[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv[1:]
    try:
        binary = binary_path()
    except FileNotFoundError as exc:
        print(f"dirsql: {exc}", file=sys.stderr)
        return 1

    if is_windows():
        completed = subprocess.run([binary, *argv])
        return completed.returncode
    os.execv(binary, [binary, *argv])
    return 0  # unreachable
