"""Assert the precheck build matrix actually produced every artifact (#790).

`release-precheck.yml` runs the same build matrix as the release, but has no
publish job -- so it never reaches the artifact-completeness check that publish
performs. `actions/upload-artifact` only *warns* when its path matches nothing,
so a build row that stages into the wrong directory uploads nothing and still
reports success. #776, #777 and #778 were each green that way while leaving
`main` unpublishable; the failure surfaced only after merge, at release.

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
import sys
import tomllib
from collections.abc import Callable, Iterable


def declared_targets(config: dict) -> list[tuple[str, str]]:
    """(package name, target triple) for every package that declares targets."""
    pairs = []
    for package in config.get("package", []):
        name = package.get("name")
        for target in package.get("targets", []):
            triple = target if isinstance(target, str) else target.get("triple")
            if name and triple:
                pairs.append((name, triple))
    return pairs


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


def read_config(path: str) -> dict:
    with open(path, "rb") as handle:
        return tomllib.load(handle)


def subdirectories(dist_dir: str) -> list[str]:
    if not os.path.isdir(dist_dir):
        return []
    return sorted(e for e in os.listdir(dist_dir) if os.path.isdir(os.path.join(dist_dir, e)))


def warn(line: str) -> None:
    print(line, file=sys.stderr)


def run(
    dist_dir: str,
    config_path: str,
    *,
    config: Callable[[str], dict] = read_config,
    entries: Callable[[str], list[str]] = subdirectories,
    walk: Callable[[str], Iterable] = os.walk,
    echo: Callable[[str], None] = warn,
) -> int:
    expected = declared_targets(config(config_path))
    found = entries(dist_dir)
    built = built_packages(expected, found)
    for name in sorted({n for n, _ in expected if n not in built}):
        # Logged, never silent: if a package you expected to be built shows up
        # here, the plan did not build it and this check asserted nothing.
        echo(f"skip artifact-completeness: {name} -- the plan built no artifacts for it")
    problems = missing(dist_dir, expected, found, walk)
    for problem in problems:
        echo(f"incomplete artifact -- {problem}")
    if problems:
        echo(
            f"artifact-completeness: {len(problems)} of the built "
            f"(package, target) pairs produced no usable artifact in {dist_dir}. "
            "A build row that stages into the wrong directory uploads nothing and "
            "still reports success, so publish would fail after merge. Check the "
            "row's build script stages where the engine packages from."
        )
        return 1
    checked = len([name for name, _ in expected if name in built])
    echo(f"ok artifact-completeness: all {checked} built (package, target) pairs present")
    return 0
