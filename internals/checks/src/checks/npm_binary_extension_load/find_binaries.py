"""Locate the bundled CLI inside the downloaded artifact tree."""

from __future__ import annotations

import os

BIN_NAME = "dirsql"


def find_binaries(dist_dir: str, walker=os.walk) -> list[str]:
    return sorted(
        os.path.join(parent, name)
        for parent, _dirs, names in walker(dist_dir)
        for name in names
        if name == BIN_NAME
    )
