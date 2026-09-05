"""Read a workflow file (#781)."""

from __future__ import annotations


def read_text(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()
