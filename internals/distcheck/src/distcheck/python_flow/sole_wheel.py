"""Picks the one `.whl` the build was supposed to emit."""
from __future__ import annotations

from .errors import DistcheckError


def sole_wheel(names) -> str:
    """The single `.whl` among `names`, or raise -- the build must emit one."""
    wheels = [name for name in names if name.endswith(".whl")]
    if len(wheels) != 1:
        raise DistcheckError(f"expected exactly one wheel, saw {wheels}")
    (wheel,) = wheels
    return wheel
