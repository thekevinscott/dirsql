"""Where the plugin persists work it does not want to repeat between scans.

A scan re-reads every matched file, so expensive per-file work (PDF text
extraction) is written here instead. The location follows the XDG cache
convention a CLI is expected to honor, and ``DIRSQL_EMBEDDINGS_CACHE_DIR``
overrides it outright -- the hook runs as a subprocess, so an environment
variable is the only channel an operator (or a test) has to redirect it.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

import os
from pathlib import Path

ENV_CACHE_DIR = "DIRSQL_EMBEDDINGS_CACHE_DIR"
ENV_XDG_CACHE_HOME = "XDG_CACHE_HOME"
CACHE_NAME = "dirsql-plugin-embeddings"


def cache_dir() -> Path:
    override = os.environ.get(ENV_CACHE_DIR)
    if override:
        return Path(override)
    xdg = os.environ.get(ENV_XDG_CACHE_HOME)
    root = Path(xdg) if xdg else Path.home() / ".cache"
    return root / CACHE_NAME
