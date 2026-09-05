"""Orchestration for the artifact-completeness check (#790).

Reads the release config, lists what the run staged, and holds the two to the
`(package, target)` invariant in `gate.py`.
"""

from __future__ import annotations

import os
from collections.abc import Callable, Iterable

from .gate import (
    built_packages,
    declared_targets,
    missing,
    read_config,
    subdirectories,
    warn,
)


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
