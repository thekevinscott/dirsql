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


def missing(
    dist_dir: str,
    expected: list[tuple[str, str]],
    entries: list[str],
    walk: Callable[[str], Iterable],
) -> list[str]:
    """One `<package> / <target>: <reason>` line per target with no usable artifact."""
    problems = []
    for name, triple in expected:
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
    config: Callable[[str], dict] | None = None,
    entries: Callable[[str], list[str]] | None = None,
    walk: Callable[[str], Iterable] = os.walk,
    echo: Callable[[str], None] | None = None,
) -> int:
    # Sentinels rather than direct defaults: a default binds the function object
    # at def time, so patching the module attribute in a test would not take.
    read = config or read_config
    listing = entries or subdirectories
    say = echo or warn
    expected = declared_targets(read(config_path))
    problems = missing(dist_dir, expected, listing(dist_dir), walk)
    for problem in problems:
        say(f"incomplete artifact -- {problem}")
    if problems:
        say(
            f"artifact-completeness: {len(problems)} of {len(expected)} declared "
            f"(package, target) pairs produced no usable artifact in {dist_dir}. "
            "A build row that stages into the wrong directory uploads nothing and "
            "still reports success, so publish would fail after merge. Check the "
            "row's build script stages where the engine packages from."
        )
        return 1
    say(f"ok artifact-completeness: all {len(expected)} declared (package, target) pairs present")
    return 0
