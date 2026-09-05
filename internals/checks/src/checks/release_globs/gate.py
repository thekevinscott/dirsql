"""Orchestration for the release-globs check (#944).

Reads the two files that between them decide what a merge to `main` publishes --
`putitoutthere.toml`'s per-package `globs` and `release-ci.yml`'s PR path filter
-- and holds them to one invariant: **everything the publish globs match, the
precheck also builds, and neither reaches a path that never ships**.

Both halves are compared by name, never by re-matching paths, so this check
carries no second copy of putitoutthere's glob semantics to drift against.
"""

from __future__ import annotations

from collections.abc import Callable

from .glob_problems import glob_problems
from .precheck import unprechecked
from .pull_request_paths import pull_request_paths
from .read_config import read_config
from .read_workflow import read_workflow


def exclusions(paths) -> list[str]:
    return [path for path in paths if path.startswith("!")]


def run(
    config_path: str,
    workflow_path: str,
    *,
    config: Callable[[str], dict] = read_config,
    workflow: Callable[[str], dict] = read_workflow,
    echo: Callable[[str], None] = print,
) -> int:
    problems = glob_problems(config(config_path).get("package", []))
    problems += unprechecked(exclusions(pull_request_paths(workflow(workflow_path))))
    for problem in problems:
        echo(f"::error::{problem}")
    if problems:
        echo(
            f"release-globs: {len(problems)} problem(s). See "
            f"internals/checks/src/checks/release_globs/decide.py for why "
            f"leading-`!` negations do not work and what the extglob carve-out "
            f"form is."
        )
        return 1
    echo(f"ok release-globs: {config_path} and {workflow_path} agree on what ships.")
    return 0
