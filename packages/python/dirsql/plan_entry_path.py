"""The loadable path one planned extension entry resolves to."""

from __future__ import annotations

import os

from .resolve_package import _resolve_package


def _plan_entry_path(entry):
    """Resolve a planned entry to a concrete file.

    A literal entry is already resolved. A package entry prefers a same-named
    file shadowing it next to the config, then the installed package.
    """
    if entry.path is not None:
        return entry.path
    if os.path.isfile(entry.shadow):
        return entry.shadow
    return _resolve_package(entry.package)
