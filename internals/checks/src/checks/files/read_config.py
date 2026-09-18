"""Parse a TOML config file."""

from __future__ import annotations

import tomllib


def read_config(path: str) -> dict:
    with open(path, "rb") as handle:
        return tomllib.load(handle)
