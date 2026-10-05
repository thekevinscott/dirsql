"""The `[package].version` literal committed in a crate manifest."""

from __future__ import annotations

from collections.abc import Callable

from ..files.read_config import read_config


def committed_version(manifest: str, config: Callable[[str], dict] = read_config) -> str:
    return config(manifest)["package"]["version"]
