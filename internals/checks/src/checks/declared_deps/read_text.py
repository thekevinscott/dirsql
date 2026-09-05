"""Source reading for the declared-deps check (#782)."""

from __future__ import annotations


def read_text(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()
