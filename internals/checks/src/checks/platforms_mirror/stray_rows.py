"""Python rows for targets the release never publishes."""

from __future__ import annotations

from .vocabulary import PYTHON_FILE, TYPESCRIPT_FILE, key


def stray_rows(typescript_keys, python_rows) -> list[str]:
    """One message per platforms.py row that is not a published target."""
    return [
        f"{key(row['node_platform'], row['node_arch'])} has a row in platforms.py "
        f"but is not a published target in platforms.ts. Either add the target to "
        f"PLATFORMS in {TYPESCRIPT_FILE} or drop the row from {PYTHON_FILE}: "
        f"distcheck must not resolve a sub-package the release never publishes."
        for row in python_rows
        if key(row["node_platform"], row["node_arch"]) not in typescript_keys
    ]
