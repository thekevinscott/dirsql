"""Platform check used by the launcher. Isolated so tests can override it
without mutating the ``os.name`` global (which upsets pathlib)."""

from __future__ import annotations

import os


def is_windows() -> bool:
    return os.name == "nt"
