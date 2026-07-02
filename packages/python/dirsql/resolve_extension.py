"""Resolve an extension entry's ``path`` to a concrete loadable file.

#225 supports only literal file paths. #298 adds resolving a bare **package
name**: when ``path`` carries no path separator and no loadable-file suffix, it
names a package installed in the runtime env, and dirsql discovers the loadable
file *inside* that package.

Resolution is an ordered probe (file-first, then package), so every literal
path from #225 keeps its old behavior and only a bare name reaches the package
machinery:

1. **Path-looking** (contains a separator, or ends in ``.so`` / ``.dylib`` /
   ``.dll`` / ``.pyd``) -- returned as a file path: made absolute against
   ``base`` when ``resolve_relative`` is set (config-file entries), else
   verbatim (programmatic entries, mirroring the Rust builder).
2. **Bare name** -- a same-named local file under ``base`` *shadows* the
   package (parity with #225's file-first probe); otherwise the package dir is
   located via :func:`importlib.util.find_spec` and the current platform's
   loadable is globbed from inside it. Zero matches and multiple matches are
   both hard errors -- the caller must disambiguate with a literal path.
"""

import glob as _glob
import importlib.util
import os
import sys

# Suffixes that mark a value as "already a file path" (so package resolution is
# never attempted) and, per platform, the globs used to find a loadable inside
# a package directory.
_LOADABLE_SUFFIXES = (".so", ".dylib", ".dll", ".pyd")


def _platform_patterns():
    """Loadable-file glob(s) for the current platform."""
    if sys.platform == "darwin":
        return ("*.dylib",)
    if sys.platform == "win32":
        return ("*.dll", "*.pyd")
    return ("*.so",)


def is_bare_name(path):
    """True when ``path`` is a bare package name rather than a file path."""
    if os.sep in path or (os.altsep and os.altsep in path):
        return False
    return not path.endswith(_LOADABLE_SUFFIXES)


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

    pat_desc = " / ".join(patterns)
    if not found:
        raise ValueError(
            f"no loadable extension file ({pat_desc}) found in package "
            f"{name!r} (searched {', '.join(dirs)})"
        )
    if len(found) > 1:
        raise ValueError(
            f"multiple loadable extension files found in package {name!r}: "
            f"{', '.join(found)}; disambiguate with a literal path"
        )
    return found[0]


def resolve_extension_path(path, base, resolve_relative):
    """Resolve an extension ``path`` to a concrete file.

    ``base`` is the directory a relative path and the bare-name shadow probe
    resolve against (a config file's parent dir, or the cwd for programmatic
    entries). ``resolve_relative`` makes a relative path-looking value absolute
    against ``base`` (config-file semantics); when false it is returned verbatim
    (programmatic semantics).
    """
    if not is_bare_name(path):
        if resolve_relative and not os.path.isabs(path):
            return os.path.join(base, path)
        return path
    local = os.path.join(base, path)
    if os.path.isfile(local):
        return local
    return _resolve_package(path)
