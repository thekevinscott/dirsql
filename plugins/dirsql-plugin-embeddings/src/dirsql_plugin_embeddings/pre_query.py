"""``pre-query`` console script: turn a ``{"q": ...}`` body into search SQL.

Accepts both a verbatim server body (``{"q": ...}``) and the CLI ``query``
subcommand's ``{"sql": <arg>}`` wrapper, embeds the question, and prints the
nearest-neighbor SQL over the ``documents`` table (ordered by
``vec_distance_cosine``). The hook owns SQL safety: the only interpolated value
is a numeric vector this script produced.

Annotations are evaluated at runtime (no ``from __future__ import annotations``)
so a mutated ``X | None`` union in a signature fails at import rather than
surviving as an inert string.
"""

import json
import sys

from .embedder import embed

TABLE_NAME = "documents"
RESULT_LIMIT = 3


def question(raw_body: str) -> str:
    body = json.loads(raw_body)
    if "q" in body:
        return body["q"]
    return json.loads(body["sql"])["q"]


def build_sql(vector: list[float]) -> str:
    needle = json.dumps(vector)
    return (
        f"SELECT path, ROUND(vec_distance_cosine(embedding, '{needle}'), 3) "
        f"AS distance FROM {TABLE_NAME} ORDER BY distance LIMIT {RESULT_LIMIT}"
    )


def main(argv: list[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv
    print(build_sql(embed(question(argv[1]))))
    return 0
