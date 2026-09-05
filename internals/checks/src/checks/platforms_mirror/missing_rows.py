"""Published targets with no row on the Python side."""

from __future__ import annotations

from .vocabulary import PYTHON_FILE, key, slug


def missing_rows(python_keys, typescript_rows) -> list[str]:
    """One message per published target with no row in platforms.py."""
    return [
        f"{key(row['nodePlatform'], row['nodeArch'])} is published by platforms.ts "
        f"but has no row in platforms.py. Add "
        f"Platform({row['nodePlatform']!r}, {row['nodeArch']!r}, "
        f"{slug(str(row.get('libName', '')))!r}, {row.get('os')!r}, "
        f"{row.get('cpu')!r}) to PLATFORMS in {PYTHON_FILE}, or the node distcheck "
        f"flow cannot resolve the new target on that host."
        for row in typescript_rows
        if key(row["nodePlatform"], row["nodeArch"]) not in python_keys
    ]
