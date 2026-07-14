"""Discover installed plugins' fragment paths, ordered by entry-point name."""

from __future__ import annotations

from importlib import metadata

from .fragment_path import fragment_path

ENTRY_POINT_GROUP = "dirsql"


def discovered_fragments() -> list[str]:
    """Fragment paths for every installed plugin, ordered by entry-point name
    (deterministic, so a running server's ``-c`` list is reproducible)."""
    entry_points = sorted(
        metadata.entry_points(group=ENTRY_POINT_GROUP), key=lambda ep: ep.name
    )
    return [fragment_path(ep.value) for ep in entry_points]
