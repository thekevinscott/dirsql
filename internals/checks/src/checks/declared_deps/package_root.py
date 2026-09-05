"""Package-root resolution for the declared-deps check (#782)."""

from __future__ import annotations

import os.path
from collections.abc import Callable


def package_root(source: str, exists: Callable[[str], bool] = os.path.exists) -> str:
    """Nearest ancestor of `source` (inclusive) holding a pyproject.toml."""
    parts = source.split("/")
    while parts:
        candidate = "/".join(parts)
        if exists(f"{candidate}/pyproject.toml"):
            return candidate
        parts.pop()
    return "."
