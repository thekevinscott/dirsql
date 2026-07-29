"""The entry point's single read seam: everything the hook reads comes through
here, so per-format handling has one place to land without touching `on_file`.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

from .read_text import read_text


def read_content(path: str) -> str:
    return read_text(path)
