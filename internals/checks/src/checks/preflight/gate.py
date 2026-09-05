"""The real-world seams `run` is handed: a subprocess runner and a config reader (#781)."""

from __future__ import annotations

import os.path
import subprocess
import tomllib
from collections.abc import Sequence


def default_runner(argv: Sequence[str], cwd: str) -> int:
    return subprocess.run(argv, cwd=cwd, check=False).returncode


def read_e2e(config: str) -> dict:
    """The `[e2e]` table of a root's testing-conventions config, if it has one."""
    if not config or not os.path.exists(config):
        return {}
    with open(config, "rb") as handle:
        return tomllib.load(handle).get("e2e", {})
