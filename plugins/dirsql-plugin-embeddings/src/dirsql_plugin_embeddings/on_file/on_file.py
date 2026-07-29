"""The ``on-file`` hook entry point.

Reads the file at ``argv[1]``, embeds its text, and prints a one-line JSON row
array (``path``, ``text``, ``embedding``).

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

import json
import sys

from ..embedder import embed
from .build_rows import build_rows
from .read_text import read_text


def on_file(argv: list[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv
    path = argv[1]
    text = read_text(path)
    print(json.dumps(build_rows(path, text, embed(text))))
    return 0
