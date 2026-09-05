"""Reading the workflow whose path filter gates the release build precheck (#944)."""

from __future__ import annotations

import yaml


def read_workflow(path: str) -> dict:
    with open(path, encoding="utf-8") as handle:
        return yaml.safe_load(handle)
