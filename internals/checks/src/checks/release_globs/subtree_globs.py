"""The wildcard entries of one package subtree (#944)."""

from __future__ import annotations

from .decide import is_wildcard
from .subtree_root import subtree_root


def subtree_globs(globs, root: str) -> list[str]:
    """Sorted wildcard entries that reach into ``root``, negations excluded."""
    return sorted(
        glob
        for glob in globs
        if not glob.startswith("!") and is_wildcard(glob) and subtree_root(glob) == root
    )
