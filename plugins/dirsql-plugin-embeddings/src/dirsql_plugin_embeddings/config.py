"""Cache configuration: what the plugin caches, where, and for how long.

Annotations are evaluated at runtime (no ``from __future__ import
annotations``) so a mutated ``X | None`` union in a signature fails at import
rather than surviving as an inert string.
"""

import os
from datetime import timedelta
from pathlib import Path

PLUGIN_NAME = "dirsql-plugin-embeddings"

CACHE_DIR = Path.home() / ".cache" / PLUGIN_NAME

# Every cache here keys on the source file's mtime, so a hit can never be
# stale and expiry would only re-do work for a file nothing has touched.
# cachetta's own default is 7 days.
CACHE_DURATION = timedelta(days=365)

# Set to "0" to make every call recompute. Writes still happen -- it is reads
# that would otherwise let one test run's leftovers decide the next one's
# result, and the hooks are subprocesses, so an inherited environment variable
# is what reaches them.
ENV_CACHE_READ = "DIRSQL_EMBEDDINGS_CACHE_READ"


def cache_reads_enabled() -> bool:
    return os.environ.get(ENV_CACHE_READ, "1") != "0"
