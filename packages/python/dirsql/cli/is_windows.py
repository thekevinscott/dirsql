"""Platform check used by the launcher."""

from __future__ import annotations

import os


def is_windows() -> bool:
    return os.name == "nt"
