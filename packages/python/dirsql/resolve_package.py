"""Location of an extension package's loadable file for this platform.

``importlib`` finds the package's directories; the core picks the one loadable
file inside them (and raises on zero or several -- the caller must then
disambiguate with a literal path).
"""

import glob as _glob
import importlib.util
import os

from ._dirsql import select_loadable


def _resolve_package(name):
    """Locate ``name``'s package dir and pick its platform loadable file."""
    try:
        spec = importlib.util.find_spec(name)
    except (ImportError, ValueError) as exc:
        raise ValueError(
            f"could not resolve extension package {name!r}: {exc}"
        ) from exc
    if spec is None:
        raise ValueError(f"could not resolve extension package {name!r}: not installed")

    dirs = list(spec.submodule_search_locations or [])
    if not dirs and spec.origin and spec.origin not in ("built-in", "frozen"):
        dirs.append(os.path.dirname(spec.origin))
    if not dirs:
        raise ValueError(
            f"could not resolve extension package {name!r}: no package directory"
        )

    candidates = []
    for d in dirs:
        candidates.extend(_glob.glob(os.path.join(d, "**", "*"), recursive=True))
    return select_loadable(name, dirs, candidates)
