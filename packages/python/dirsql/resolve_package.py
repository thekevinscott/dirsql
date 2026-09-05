"""Location of an extension package's loadable file for this platform.

Zero matches and multiple matches are both hard errors -- the caller must
disambiguate with a literal path.
"""

import glob as _glob
import importlib.util
import os

from .platform_patterns import _platform_patterns


def _resolve_package(name):
    """Locate ``name``'s package dir and glob its platform loadable file."""
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

    patterns = _platform_patterns()
    matches = set()
    for d in dirs:
        for pat in patterns:
            matches.update(_glob.glob(os.path.join(d, "**", pat), recursive=True))
    found = sorted(matches)

    try:
        (single,) = found
    except ValueError:
        pat_desc = " / ".join(patterns)
        if not found:
            raise ValueError(
                f"no loadable extension file ({pat_desc}) found in package "
                f"{name!r} (searched {', '.join(dirs)})"
            ) from None
        raise ValueError(
            f"multiple loadable extension files found in package {name!r}: "
            f"{', '.join(found)}; disambiguate with a literal path"
        ) from None
    return single
