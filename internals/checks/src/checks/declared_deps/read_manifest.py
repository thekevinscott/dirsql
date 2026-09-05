"""Manifest reading for the declared-deps check (#782)."""

from __future__ import annotations

import tomllib


def read_manifest(path: str) -> dict:
    with open(path, "rb") as handle:
        return tomllib.load(handle)
