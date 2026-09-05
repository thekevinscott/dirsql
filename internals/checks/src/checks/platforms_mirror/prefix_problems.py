"""Published targets whose `libName` no `slug` can be derived from."""

from __future__ import annotations

from .vocabulary import LIB_PREFIX, TYPESCRIPT_FILE, key


def prefix_problems(rows) -> list[str]:
    """One message per `libName` that `librarySlug()` would throw on."""
    return [
        f"{key(row.get('nodePlatform'), row.get('nodeArch'))}: libName "
        f"{row.get('libName')!r} does not start with {LIB_PREFIX!r}, so "
        f"`librarySlug()` throws on it and the Python `slug` cannot be derived. "
        f"Fix the name in {TYPESCRIPT_FILE}."
        for row in rows
        if not str(row.get("libName", "")).startswith(LIB_PREFIX)
    ]
