"""Test-file discovery for the pytest-gate check (#494/#495)."""
from __future__ import annotations

from pathlib import Path


def find_test_files(paths):
    matches = []
    for path in paths:
        matches.extend(Path(path).rglob("*_test.py"))
    return matches
