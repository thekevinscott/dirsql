"""Orchestration for the release-globs check (#944).

Reads the two files that between them decide what a merge to `main` publishes --
`putitoutthere.toml`'s per-package `globs` and `release-ci.yml`'s PR path filter
-- and holds them to one invariant: **everything the publish globs match, the
precheck also builds, and neither reaches a path that never ships**.

Both halves are compared by name, never by re-matching paths, so this check
carries no second copy of putitoutthere's glob semantics to drift against.
"""

from __future__ import annotations

import tomllib
from collections.abc import Callable

import yaml

from .decide import glob_problems
from .precheck import unprechecked


def read_config(path: str) -> dict:
    with open(path, "rb") as handle:
        return tomllib.load(handle)


def read_workflow(path: str) -> dict:
    with open(path, encoding="utf-8") as handle:
        return yaml.safe_load(handle)


def pull_request_paths(workflow: dict) -> list[str]:
    """The `on.pull_request.paths` filter, or empty when the workflow has none.

    YAML 1.1 resolves a bare ``on`` key to the boolean ``True``, which is how
    PyYAML hands back every GitHub workflow; the string key is accepted too so a
    quoted ``"on":`` reads the same.
    """
    triggers = workflow.get(True, workflow.get("on")) or {}
    return (triggers.get("pull_request") or {}).get("paths") or []


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
