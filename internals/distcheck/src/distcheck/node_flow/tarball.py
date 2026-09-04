"""Pick the one packed tarball a distcheck stage means to install."""
from __future__ import annotations

from typing import Optional

from .errors import DistcheckError


def select_tarball(names, prefix: str, exclude: Optional[str] = None) -> str:
    """The single `.tgz` in `names` matching `prefix` (and not `exclude`)."""
    matches = [
        name
        for name in names
        if name.startswith(prefix)
        and name.endswith(".tgz")
        and (exclude is None or not name.startswith(exclude))
    ]
    if len(matches) != 1:
        raise DistcheckError(
            f"expected exactly one {prefix!r} tarball, saw {matches} in {list(names)}"
        )
    (only,) = matches
    return only
