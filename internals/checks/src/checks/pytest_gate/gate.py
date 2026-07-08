"""Pure logic for the pytest-gate check (#494/#495).

Translates pytest's "no tests collected" exit code to success, so a directory with no
`*_test.py` files yet (or no longer any) passes cleanly instead of failing CI.
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

NO_TESTS_COLLECTED = 5


def find_test_files(paths):
    matches = []
    for path in paths:
        matches.extend(Path(path).rglob("*_test.py"))
    return matches


def interpret(returncode):
    if returncode == NO_TESTS_COLLECTED:
        return 0
    return returncode


def run(argv, runner=subprocess.run, finder=find_test_files):
    paths = [arg for arg in argv if not arg.startswith("-")]
    if not finder(paths):
        print(f"No *_test.py under {paths or ['.']} — nothing to test.")
        return 0
    result = runner([sys.executable, "-m", "pytest", *argv])
    return interpret(result.returncode)
