import os
from datetime import timedelta
from pathlib import Path

from cachetta import Cachetta

# No eviction: entries stay until the user wipes the directory (documented as
# safe -- the only cost is re-embedding).
NO_EVICTION = timedelta(days=365000)


def cache_dir():
    xdg = os.environ.get("XDG_CACHE_HOME", "")
    base = Path(xdg) if xdg else Path.home() / ".cache"
    return base / "dirsql" / "embeddings"


def make_cache():
    return Cachetta(path=cache_dir(), hashed=True, duration=NO_EVICTION)
