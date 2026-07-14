"""``on-file`` console script: embed one matched file into a dirsql row.

RED stub: signatures only; behavior is unimplemented so the colocated unit
tests fail their assertions until the GREEN commit fills them in.
"""

from __future__ import annotations

import sys

from .embedder import embed


def _read_text(path: str) -> str:
    return ""


def build_rows(path: str, text: str, vector: list[float]) -> list[dict]:
    return []


def main(argv: list[str] | None = None) -> int:
    return 1
