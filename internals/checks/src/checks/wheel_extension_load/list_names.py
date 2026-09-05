"""Directory listing that treats an absent artifact dir as empty."""

from __future__ import annotations

import os


def list_names(dist_dir: str, listdir=os.listdir) -> list[str]:
    try:
        return list(listdir(dist_dir))
    except FileNotFoundError:
        return []
