"""The package subtrees a glob list reaches into (#944)."""

from __future__ import annotations

from .decide import is_wildcard
from .subtree_root import subtree_root


def subtree_roots(globs) -> list[str]:
    """Sorted package subtrees the non-negated wildcard entries reach into."""
    roots = {
        root
        for glob in globs
        if not glob.startswith("!")
        and is_wildcard(glob)
        and (root := subtree_root(glob)) is not None
    }
    return sorted(roots)
