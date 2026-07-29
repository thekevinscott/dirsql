"""Extract a PDF's text.

pypdf rather than pymupdf: pymupdf is AGPL-3.0, and a runtime dependency of
this MIT-licensed package propagates to everyone who installs it.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

from pypdf import PdfReader


def read_pdf(path: str) -> str:
    reader = PdfReader(path)
    return "\n".join(page.extract_text() for page in reader.pages)
