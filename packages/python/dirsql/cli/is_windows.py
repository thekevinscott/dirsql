"""Platform check used by the launcher. Pure function of an OS name; the
default reads ``os.name`` so callers needn't pass anything in production.
Tests pass an explicit string instead of mutating ``os.name`` (which
upsets pathlib)."""

from __future__ import annotations

import os


def is_windows(os_name: str = os.name) -> bool:
    return os_name == "nt"
