"""The failure type every node distcheck stage raises.

Its own module so `tarball.py` can raise it without importing `gate.py`, which
imports `tarball.py`.
"""
from __future__ import annotations


class DistcheckError(RuntimeError):
    """A distcheck stage failed -- carries a human-readable diagnostic."""
