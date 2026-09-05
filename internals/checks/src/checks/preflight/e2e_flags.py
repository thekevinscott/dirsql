"""Render a root's `[e2e]` config table as CLI flags (#781).

`e2e verify` takes no `--config`, so the table has to arrive as flags.
"""

from __future__ import annotations


def e2e_flags(e2e: dict) -> list[str]:
    flags = []
    for scope in e2e.get("extra_scope", []):
        flags += ["--extra-scope", scope]
    for path in e2e.get("exclude", []):
        flags += ["--exclude", path]
    return flags
