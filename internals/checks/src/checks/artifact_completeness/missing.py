"""Hold each declared (package, target) to a non-empty artifact (#790).

**This matches on package name + target triple, never on the engine's artifact
naming.** Both of those are declared in our own `putitoutthere.toml`; the
`{mode}` segment is the engine's, and it is exactly what shifted under us in
#788 (two npm build rows produced `dirsql-npm-napi-<triple>`, one produces
`dirsql-npm-<triple>`). A check that re-derived the full convention would carry
its own copy of that rule and drift the same way -- it would have had the bug it
is meant to catch. Matching the halves we own is robust to the engine renaming
its segments, and still fails loudly when a target produces nothing at all.
"""

from __future__ import annotations

import os
from collections.abc import Callable, Iterable


def populated(directory: str, walk: Callable[[str], Iterable]) -> bool:
    """True when `directory` holds at least one file at any depth."""
    return any(files for _root, _dirs, files in walk(directory))


def built_packages(expected: list[tuple[str, str]], entries: list[str]) -> set[str]:
    """Packages the plan actually built this run.

    The precheck matrix only builds packages whose globs the PR touched, so a
    PR that changes no shipped source legitimately produces nothing. A package
    with no artifact at all was not planned; a package with *some* artifacts but
    missing targets is the #788 signature and must fail.
    """
    return {name for name, _ in expected if any(name in entry for entry in entries)}


def missing(
    dist_dir: str,
    expected: list[tuple[str, str]],
    entries: list[str],
    walk: Callable[[str], Iterable],
) -> list[str]:
    """One `<package> / <target>: <reason>` line per target with no usable artifact."""
    built = built_packages(expected, entries)
    problems = []
    for name, triple in expected:
        if name not in built:
            continue
        matches = [e for e in entries if name in e and triple in e]
        if not matches:
            problems.append(f"{name} / {triple}: no artifact directory matching *{triple}*")
        elif not any(populated(os.path.join(dist_dir, m), walk) for m in matches):
            joined = ", ".join(sorted(matches))
            problems.append(f"{name} / {triple}: artifact present but empty ({joined})")
    return problems
