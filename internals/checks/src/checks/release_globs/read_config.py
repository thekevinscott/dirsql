"""Reading the release config that declares each package's publish globs (#944)."""

from __future__ import annotations

import tomllib


def read_config(path: str) -> dict:
    with open(path, "rb") as handle:
        return tomllib.load(handle)
