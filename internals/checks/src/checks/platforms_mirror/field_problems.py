"""Per-row field comparison for the platforms-mirror check (#1004).

Every field in `decide.py`'s SHARED must hold the same value on both sides of a
target that exists in both tables.
"""

from __future__ import annotations

from .decide import PYTHON_FILE, SHARED, key, typescript_value


def field_problems(python_row: dict, typescript_row: dict) -> list[str]:
    """One message per shared field whose two values disagree."""
    problems = []
    for field in SHARED:
        found = python_row.get(field)
        expected = typescript_value(field, typescript_row)
        if found != expected:
            problems.append(
                f"{key(python_row['node_platform'], python_row['node_arch'])}: "
                f"{field} is {found!r} in platforms.py, {expected!r} in "
                f"platforms.ts. platforms.ts is the release source of truth -- "
                f"change {PYTHON_FILE} to match."
            )
    return problems
