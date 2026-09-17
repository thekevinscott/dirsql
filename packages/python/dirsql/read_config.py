"""Reading one TOML config off disk for the core's extension planner."""

from __future__ import annotations


def _read_config(config_path):
    """Return the config's text, or ``None`` when it cannot be read.

    A missing or unreadable config is left for the core to report.
    """
    try:
        with open(config_path, encoding="utf-8") as f:
            return f.read()
    except OSError:
        return None
