"""Find the package a gate must run from (#781)."""

from __future__ import annotations

from collections.abc import Callable

MANIFESTS = ("pyproject.toml", "package.json", "Cargo.toml")


def package_root(source: str, exists: Callable[[str], bool]) -> str:
    """Nearest ancestor of `source` (inclusive) holding a package manifest."""
    parts = source.split("/")
    while parts:
        candidate = "/".join(parts)
        if any(exists(f"{candidate}/{name}") for name in MANIFESTS):
            return candidate
        parts.pop()
    return "."
