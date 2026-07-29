"""Shape one embedded file into the command hook's row array.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

import json


def build_rows(path: str, text: str, vector: list[float]) -> list[dict]:
    # The embedding is stored as JSON text, which `sqlite-vec` accepts directly.
    return [{"path": path, "text": text, "embedding": json.dumps(vector)}]
