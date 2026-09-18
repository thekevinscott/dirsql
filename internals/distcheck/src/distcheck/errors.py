"""The failure type every distcheck stage raises, in both flows.

Its own module so a stage helper can raise it without importing the `gate.py`
that imports the helper.
"""
from __future__ import annotations


class DistcheckError(RuntimeError):
    """A distcheck stage failed -- carries a human-readable diagnostic."""
