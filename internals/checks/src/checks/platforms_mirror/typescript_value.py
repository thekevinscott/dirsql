"""One TypeScript row's value for a Python field name."""

from __future__ import annotations

from .vocabulary import DERIVE, SHARED


def typescript_value(field: str, row: dict):
    """``row``'s value for a Python field name."""
    value = row.get(SHARED[field])
    derive = DERIVE.get(field)
    if derive is None or not isinstance(value, str):
        return value
    return derive(value)
