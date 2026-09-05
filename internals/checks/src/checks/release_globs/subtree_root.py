"""Which published package subtree a glob reaches into (#944)."""

from __future__ import annotations

from .decide import PUBLISHED_ROOTS


def subtree_root(glob: str):
    """``<root>/<package>`` when ``glob`` reaches into a published package tree,
    else ``None``."""
    parts = glob.split("/")
    if len(parts) < 3 or parts[0] not in PUBLISHED_ROOTS:
        return None
    return f"{parts[0]}/{parts[1]}"
