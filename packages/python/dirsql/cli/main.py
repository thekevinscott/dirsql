"""Console-script entry point. Execs the bundled binary on POSIX,
subprocesses it on Windows."""

from __future__ import annotations

import os
import subprocess
import sys
from typing import Any, Callable, TextIO

from dirsql.cli.binary_path import binary_path as _default_binary_path
from dirsql.cli.is_windows import is_windows as _default_is_windows


def main(
    argv: list[str] | None = None,
    *,
    binary_path_fn: Callable[[], str] = _default_binary_path,
    is_windows_fn: Callable[[], bool] = _default_is_windows,
    subprocess_run: Callable[..., Any] = subprocess.run,
    execv: Callable[[str, list[str]], None] = os.execv,
    stderr: TextIO = sys.stderr,
) -> int:
    if argv is None:
        argv = sys.argv[1:]
    try:
        binary = binary_path_fn()
    except FileNotFoundError as exc:
        print(f"dirsql: {exc}", file=stderr)
        return 1

    if is_windows_fn():
        completed = subprocess_run([binary, *argv])
        return completed.returncode
    execv(binary, [binary, *argv])
    return 0
