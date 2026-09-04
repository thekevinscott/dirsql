"""Read the workflow `--conventions` names, or say what to do instead (#781).

Both ways the flag can miss -- a path that does not exist and a workflow that
calls nothing -- leave the matrix empty, and #973 showed what that costs when it
surfaces as a bare `FileNotFoundError`. Each carries the same fix, which is why
the read and the caller check sit in one function.
"""

from __future__ import annotations

from collections.abc import Callable

from .matrix import NoGateMatrix, REUSABLE, WORKFLOWS, parse_gate_matrix


def named(path: str, read: Callable[[str], str]) -> str:
    """The text of an explicitly named workflow, or a NoGateMatrix carrying the fix."""
    try:
        text = read(path)
    except OSError as err:
        raise NoGateMatrix(
            f"--conventions {path}: no such workflow. Name one that calls {REUSABLE}, "
            f"or drop the flag to derive the matrix from every caller in {WORKFLOWS}."
        ) from err
    if not parse_gate_matrix(text):
        raise NoGateMatrix(
            f"--conventions {path}: no job in it calls {REUSABLE}, so it declares no "
            f"gates. Name a workflow that does, or drop the flag to derive the matrix "
            f"from every caller in {WORKFLOWS}."
        )
    return text
