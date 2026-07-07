"""Console-script entry point. Execs the bundled binary on POSIX,
subprocesses it on Windows. All argv is forwarded transparently to the
bundled Rust binary."""

from __future__ import annotations

import os
import subprocess
import sys

from .binary_path import binary_path
from .is_windows import is_windows
from .resolve_config_extensions import with_resolved_extensions


def main(argv: list[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv[1:]

    try:
        binary = binary_path()
    except FileNotFoundError as exc:
        print(f"dirsql: {exc}", file=sys.stderr)
        return 1

    # Resolve any package-name extensions in a TOML config here (the binary
    # can't) and pass them as `--extension` flags; a no-op otherwise.
    try:
        argv = with_resolved_extensions(argv)
    except Exception as exc:
        print(f"dirsql: {exc}", file=sys.stderr)
        return 1

    if is_windows():
        completed = subprocess.run([binary, *argv])
        return completed.returncode
    os.execv(binary, [binary, *argv])
    return 0
