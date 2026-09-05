"""The failure type every python distcheck stage raises.

Its own module so the stage helpers can raise it without importing `gate.py`,
which imports them.
"""
from __future__ import annotations


class DistcheckError(RuntimeError):
    """A distcheck stage failed -- carries a human-readable diagnostic."""
