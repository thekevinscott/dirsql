"""The entry point's single read seam: everything the hook reads comes through
here, so per-format handling has one place to land without touching `on_file`.

The extension is matched case-insensitively, and more permissively than the
fragment's glob (which globset matches case-sensitively): the hook is a console
script runnable against any path, not only the ones a glob selected.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

import os

from .read_pdf import read_pdf
from .read_text import read_text


def read_content(path: str) -> str:
    if os.path.splitext(path)[1].lower() == ".pdf":
        return read_pdf(path)
    return read_text(path)
