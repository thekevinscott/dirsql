"""Asserts the built wheel carries the stable-ABI tag the release matrix assumes."""
from __future__ import annotations

from .errors import DistcheckError


def check_wheel_tag(wheel: str) -> None:
    """Assert the stable-ABI (abi3) tag (#487): one `cp3x-abi3` wheel per
    platform, not a version-locked `cpXY-cpXY` that re-inflates the release
    matrix 4x."""
    if "-abi3-" not in wheel:
        raise DistcheckError(f"expected an abi3 wheel tag, saw {wheel!r}")
    interp = wheel.split("-")[2]  # dirsql-<ver>-<interp>-<abi>-<plat>.whl
    if not interp.startswith("cp3"):
        raise DistcheckError(f"unexpected interpreter tag in {wheel!r}")
