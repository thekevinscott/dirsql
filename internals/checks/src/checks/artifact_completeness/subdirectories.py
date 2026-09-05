"""List what a run staged: one subdirectory per downloaded artifact (#790)."""

from __future__ import annotations

import os


def subdirectories(dist_dir: str) -> list[str]:
    if not os.path.isdir(dist_dir):
        return []
    return sorted(e for e in os.listdir(dist_dir) if os.path.isdir(os.path.join(dist_dir, e)))
