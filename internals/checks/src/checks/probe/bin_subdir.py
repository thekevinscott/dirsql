"""Where a venv keeps its console scripts, per platform."""

from __future__ import annotations

import os


def bin_subdir(os_name: str = os.name) -> str:
    return {"nt": "Scripts"}.get(os_name, "bin")
