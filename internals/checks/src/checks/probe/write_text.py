"""Write a probe's scratch config to disk."""

from __future__ import annotations


def write_text(path: str, content: str) -> None:
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(content)
