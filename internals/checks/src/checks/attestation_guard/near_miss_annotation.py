"""The annotation for a bypass line that was attempted but misspelled (#1043)."""

from __future__ import annotations


def near_miss_annotation(line: str) -> str:
    """The annotation naming a bypass line that was attempted but misspelled."""
    return (
        f"::error::{line!r} is not the bypass line. The exact form is "
        "'allow-receipt-deletion: <reason>', reason required."
    )
