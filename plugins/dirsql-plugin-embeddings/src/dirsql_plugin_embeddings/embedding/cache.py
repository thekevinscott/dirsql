import os
from pathlib import Path


def cache_dir():
    xdg = os.environ.get("XDG_CACHE_HOME", "")
    base = Path(xdg) if xdg else Path.home() / ".cache"
    return base / "dirsql" / "embeddings"
