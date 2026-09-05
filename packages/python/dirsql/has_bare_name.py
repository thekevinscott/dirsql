"""Whether any of a config's extension entries names a package."""

from __future__ import annotations

from .is_bare_name import is_bare_name


def _has_bare_name(entries):
    return any(
        isinstance(e, dict)
        and isinstance(e.get("path"), str)
        and is_bare_name(e["path"])
        for e in entries
    )
