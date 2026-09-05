"""The failure type shared by the extension-load probes."""

from __future__ import annotations


class ProbeError(RuntimeError):
    """A probe stage failed -- carries a human-readable diagnostic."""
