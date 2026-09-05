"""Python-source discovery for the declared-deps check (#782)."""

from __future__ import annotations

import os
import os.path
from collections.abc import Callable, Iterable


def source_files(source: str, walk: Callable[[str], Iterable] = os.walk) -> list[str]:
    found = []
    for directory, _subdirs, names in walk(source):
        found += [os.path.join(directory, n) for n in sorted(names) if n.endswith(".py")]
    return sorted(found)
