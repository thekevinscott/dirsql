"""The full platforms-mirror verdict (#1004).

Structural disagreements first -- an unmirrored field, a malformed `libName`, a
target on only one side -- then the per-field comparison for the targets both
tables carry.
"""

from __future__ import annotations

from .decide import unmirrored_fields
from .field_problems import field_problems
from .missing_rows import missing_rows
from .prefix_problems import prefix_problems
from .stray_rows import stray_rows
from .vocabulary import key


def problems(fields, python_rows, typescript_rows) -> list[str]:
    """Every way the two tables disagree, most structural first."""
    found = unmirrored_fields(fields) + prefix_problems(typescript_rows)
    by_key = {key(row["nodePlatform"], row["nodeArch"]): row for row in typescript_rows}
    python_keys = {key(row["node_platform"], row["node_arch"]) for row in python_rows}
    found += missing_rows(python_keys, typescript_rows)
    found += stray_rows(set(by_key), python_rows)
    for row in python_rows:
        counterpart = by_key.get(key(row["node_platform"], row["node_arch"]))
        if counterpart is not None:
            found += field_problems(row, counterpart)
    return found
