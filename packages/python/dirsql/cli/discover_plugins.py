"""Launcher-side plugin discovery: installed = active, CLI only (#363/#529).

RED skeleton: the surface exists so the colocated unit tests import and run,
but the behavior is not implemented yet -- the tests assert against these
stubs and fail. The GREEN commit fills these in.
"""

from __future__ import annotations

import os  # noqa: F401  (unit tests patch `discover_plugins.os.environ`)
from importlib import metadata, resources  # noqa: F401


def _user_passed_config(argv: list[str]) -> bool:
    return False


def _discovery_disabled(argv: list[str]) -> bool:
    return False


def _fragment_path(module_name: str) -> str:
    return ""


def _discovered_fragments() -> list[str]:
    return []


def with_discovered_plugins(argv: list[str]) -> list[str]:
    return argv
