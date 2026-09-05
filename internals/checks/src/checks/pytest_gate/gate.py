"""Pure logic for the pytest-gate check (#494/#495).

Translates pytest's "no tests collected" exit code to success, so a directory with no
`*_test.py` files yet (or no longer any) passes cleanly instead of failing CI.
"""
from __future__ import annotations

NO_TESTS_COLLECTED = 5


def interpret(returncode):
    if returncode == NO_TESTS_COLLECTED:
        return 0
    return returncode
