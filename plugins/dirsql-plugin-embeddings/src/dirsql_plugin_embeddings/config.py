"""Cache configuration: what the plugin caches, where, and for how long.

A scan re-reads every matched file, so expensive per-file work is written to
disk instead of repeated. The location follows the XDG cache convention a CLI is
expected to honor, and ``DIRSQL_EMBEDDINGS_CACHE_DIR`` overrides it outright --
the hooks run as subprocesses, so an environment variable is the only channel an
operator (or a test) has to redirect them.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

import os
from datetime import timedelta
from pathlib import Path

PLUGIN_NAME = "dirsql-plugin-embeddings"

ENV_CACHE_DIR = "DIRSQL_EMBEDDINGS_CACHE_DIR"
ENV_XDG_CACHE_HOME = "XDG_CACHE_HOME"

# Every cache here keys on the source file's mtime, so a hit can never be
# stale and expiry would only re-do work for a file nothing has touched.
# cachetta's own default is 7 days.
CACHE_DURATION = timedelta(days=365)


def cache_dir() -> Path:
    override = os.environ.get(ENV_CACHE_DIR)
    if override:
        return Path(override)
    xdg = os.environ.get(ENV_XDG_CACHE_HOME)
    root = Path(xdg) if xdg else Path.home() / ".cache"
    return root / PLUGIN_NAME
