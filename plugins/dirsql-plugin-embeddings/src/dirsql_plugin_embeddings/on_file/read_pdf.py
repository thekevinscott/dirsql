"""Extract a PDF's text, once per version of the file.

pypdf rather than pymupdf: pymupdf is AGPL-3.0, and a runtime dependency of
this MIT-licensed package propagates to everyone who installs it.

Extraction is the expensive step of a scan and a scan re-reads every matched
file, so the result is persisted to disk. The cache key is the pair
``(path, mtime)``: an edited PDF gets a different key rather than a stale hit.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

import os

from pypdf import PdfReader

from ..cache import cache


@cache / "extract"
def extract(path: str, mtime: float) -> str:
    # `mtime` is unread on purpose: it is what makes an edited PDF a cache
    # miss, since cachetta derives the key from the arguments it is called with.
    reader = PdfReader(path)
    return "\n".join(page.extract_text() for page in reader.pages)


def read_pdf(path: str) -> str:
    return extract(path, os.path.getmtime(path))
