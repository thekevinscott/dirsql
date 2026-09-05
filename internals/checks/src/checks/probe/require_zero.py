"""Turn a non-zero subprocess exit into a diagnosed probe failure."""

from __future__ import annotations

from .probe_error import ProbeError


def require_zero(result, message: str) -> None:
    if result.returncode != 0:
        raise ProbeError(message)
