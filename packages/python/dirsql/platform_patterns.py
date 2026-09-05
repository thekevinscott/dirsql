"""The loadable-file glob pattern(s) the running platform uses."""

import sys


def _platform_patterns():
    """Loadable-file glob(s) for the current platform."""
    if sys.platform == "darwin":
        return ("*.dylib",)
    if sys.platform == "win32":
        return ("*.dll", "*.pyd")
    return ("*.so",)
