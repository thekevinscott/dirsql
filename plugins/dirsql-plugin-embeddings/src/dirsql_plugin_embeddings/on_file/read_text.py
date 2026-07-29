"""Read the matched file's text off disk.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""


def read_text(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()
