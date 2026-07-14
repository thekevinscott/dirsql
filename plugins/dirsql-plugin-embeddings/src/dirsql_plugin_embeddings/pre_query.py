"""``pre-query`` console script: turn a ``{"q": ...}`` body into search SQL.

RED stub: signatures only; behavior is unimplemented so the colocated unit
tests fail their assertions until the GREEN commit fills them in.
"""

from __future__ import annotations

import sys

from .embedder import embed

TABLE_NAME = "documents"
RESULT_LIMIT = 3


def question(raw_body: str) -> str:
    return ""


def build_sql(vector: list[float]) -> str:
    return ""


def main(argv: list[str] | None = None) -> int:
    return 1
