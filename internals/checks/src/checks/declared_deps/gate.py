"""Distribution-name vocabulary for the declared-deps check (#782).

Import name and distribution name differ (`yaml` ships in `pyyaml`), so the
mapping comes from `importlib.metadata.packages_distributions()` rather than a
hand-kept table that would drift. The installed environment supplies only the
*name* mapping; whether a dependency is declared is read from the manifest.
"""

from __future__ import annotations

import sys


def normalize(name: str) -> str:
    """PEP 503 name normalization, so `bin-shim` and `bin_shim` are one name."""
    return name.lower().replace("_", "-")


def requirement_name(spec: str) -> str:
    """The distribution name from a requirement string, dropping any version/extras."""
    for separator in ("[", "<", ">", "=", "!", "~", ";", " "):
        spec = spec.split(separator)[0]
    return normalize(spec)


def providers(module: str, distributions: dict[str, list[str]]) -> set[str]:
    """Declared-name candidates for an import: its distributions, else itself."""
    return {normalize(name) for name in distributions.get(module, [module])}


def warn(line: str) -> None:
    print(line, file=sys.stderr)
