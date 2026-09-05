"""Orchestration for the pytest-gate check (#494/#495).

Skips pytest entirely when the scanned paths hold no test files, and translates its
exit code otherwise.
"""
from __future__ import annotations

import subprocess
import sys

from .find_test_files import find_test_files
from .gate import interpret


def run(argv, runner=subprocess.run, finder=find_test_files):
    paths = [arg for arg in argv if not arg.startswith("-")]
    if not finder(paths):
        print(f"No *_test.py under {paths or ['.']} — nothing to test.")
        return 0
    result = runner([sys.executable, "-m", "pytest", *argv])
    return interpret(result.returncode)
