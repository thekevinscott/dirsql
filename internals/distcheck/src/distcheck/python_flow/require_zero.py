"""The exit-code guard every python distcheck stage runs its subprocess through."""
from __future__ import annotations

from .errors import DistcheckError


def require_zero(result, message: str) -> None:
    """Raise `DistcheckError(message)` unless `result` exited 0."""
    if result.returncode != 0:
        raise DistcheckError(message)
