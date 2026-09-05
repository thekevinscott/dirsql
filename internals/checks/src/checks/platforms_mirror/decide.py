"""Python fields the mirror cannot source from TypeScript.

A field outside `SHARED` has nowhere to read its value from, so it is drift by
construction: either it was never published, or the TypeScript interface grew a
property nobody added to the shared vocabulary.
"""

from __future__ import annotations

from .vocabulary import SHARED, TYPESCRIPT_FILE


def unmirrored_fields(fields) -> list[str]:
    """One message per dataclass field with no TypeScript source."""
    return [
        f"Platform.{field} has no counterpart in {TYPESCRIPT_FILE}. platforms.py "
        f"holds a deliberate subset of the published target ({', '.join(SHARED)}), "
        f"so a field the mirror cannot source is drift: delete it, or add the "
        f"property to the TypeScript `Platform` interface and to SHARED here."
        for field in fields
        if field not in SHARED
    ]
