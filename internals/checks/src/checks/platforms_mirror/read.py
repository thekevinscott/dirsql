"""A platform table's text, off disk."""

from __future__ import annotations


def read(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()
