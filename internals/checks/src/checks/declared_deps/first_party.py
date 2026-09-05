"""First-party name discovery for the declared-deps check (#782)."""

from __future__ import annotations

import os.path
from collections.abc import Callable, Iterable


def first_party(source: str, listdir: Callable[[str], Iterable[str]] = os.listdir) -> set[str]:
    """Top-level names the scanned tree itself defines -- never a dependency."""
    names = {os.path.basename(source.rstrip("/"))}
    for entry in listdir(source):
        names.add(entry[:-3] if entry.endswith(".py") else entry)
    return names
