"""Whether plugin discovery is opted out (flag or env var)."""

from __future__ import annotations

import os

NO_PLUGIN_FLAG = "--no-plugin"
NO_PLUGIN_ENV = "DIRSQL_NO_PLUGIN"


def discovery_disabled(argv: list[str]) -> bool:
    """True when discovery is opted out via ``--no-plugin`` or
    ``DIRSQL_NO_PLUGIN``."""
    return NO_PLUGIN_FLAG in argv or bool(os.environ.get(NO_PLUGIN_ENV))
