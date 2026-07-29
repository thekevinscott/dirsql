"""Extract a PDF's text, once per version of the file.

pypdf rather than pymupdf: pymupdf is AGPL-3.0, and a runtime dependency of
this MIT-licensed package propagates to everyone who installs it.

Extraction is the expensive step of a scan and a scan re-reads every matched
file, so the result is persisted to disk. The cache key is the pair
``(path, mtime)``: an edited PDF gets a different key rather than a stale hit,
which in turn means an entry can never go stale, so the freshness window is set
far past cachetta's 7-day default -- expiring an entry would only re-extract a
file nothing has touched.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

import os
from datetime import timedelta
from pathlib import Path

from cachetta import Cachetta
from pypdf import PdfReader

from .cache_dir import cache_dir

CACHE_DURATION = timedelta(days=365)
CACHE_SUBDIR = "pdf-text"


def pdf_cache_dir(*args, **kwargs) -> Path:
    # cachetta calls a callable ``path`` with the wrapped function's own
    # arguments; the bucket is the same for every PDF, and ``hashed=True``
    # names the file within it after those arguments.
    return cache_dir() / CACHE_SUBDIR


@Cachetta(path=pdf_cache_dir, hashed=True, duration=CACHE_DURATION)
def extract(path: str, mtime: float) -> str:
    # ``mtime`` is unread on purpose: it is what makes an edited PDF a cache
    # miss, and cachetta derives the key from the arguments it is called with.
    reader = PdfReader(path)
    return "\n".join(page.extract_text() for page in reader.pages)


def read_pdf(path: str) -> str:
    return extract(path, os.path.getmtime(path))
