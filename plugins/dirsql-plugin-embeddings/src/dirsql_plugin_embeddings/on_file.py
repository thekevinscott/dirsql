"""``on-file`` console script: embed one matched file into a dirsql row.

Reads the file at ``argv[1]``, embeds its text, and prints a one-line JSON row
array (``path``, ``text``, ``embedding``) -- the embedding stored as JSON text,
which ``sqlite-vec`` accepts directly.
"""

from __future__ import annotations

import json
import sys

from .embedder import embed


def _read_text(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def build_rows(path: str, text: str, vector: list[float]) -> list[dict]:
    return [{"path": path, "text": text, "embedding": json.dumps(vector)}]


def main(argv: list[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv
    path = argv[1]
    text = _read_text(path)
    print(json.dumps(build_rows(path, text, embed(text))))
    return 0
