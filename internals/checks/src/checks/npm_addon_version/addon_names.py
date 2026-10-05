"""The napi addons among a dist directory's entries."""

from __future__ import annotations


def addon_names(names) -> list[str]:
    return sorted(name for name in names if name.endswith(".node"))
