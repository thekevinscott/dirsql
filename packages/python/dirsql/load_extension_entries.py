"""Reading one TOML config's ``[[dirsql.extension]]`` array off disk."""

from __future__ import annotations

import os

from .toml_module import _toml


def _load_extension_entries(config_path):
    """Return ``(entries, base_dir)`` for a config's ``[[dirsql.extension]]``.

    ``None`` when the config is missing, unreadable/malformed, or declares no
    extension array -- the caller should leave such configs to the core.
    """
    if not os.path.isfile(config_path):
        return None
    try:
        with open(config_path, "rb") as f:
            doc = _toml.load(f)
    except (OSError, _toml.TOMLDecodeError):
        # Leave a malformed / unreadable config for the core to report.
        return None

    entries = (doc.get("dirsql") or {}).get("extension")
    if not isinstance(entries, list):
        return None
    return entries, os.path.dirname(os.path.abspath(config_path))
