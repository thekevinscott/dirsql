"""Orchestration for the declared-deps check (#782).

Resolves the manifest beside the scanned tree's package root and reports every
import in `undeclared.py`'s list.
"""

from __future__ import annotations

from collections.abc import Callable
from importlib.metadata import packages_distributions

from .first_party import first_party
from .gate import warn
from .package_root import package_root
from .read_manifest import read_manifest
from .read_text import read_text
from .source_files import source_files
from .undeclared import undeclared


def run(
    source: str,
    *,
    manifest: Callable[[str], dict] = read_manifest,
    distributions: Callable[[], dict] = packages_distributions,
    read: Callable[[str], str] = read_text,
    files: Callable[[str], list[str]] = source_files,
    ours: Callable[[str], set[str]] = first_party,
    echo: Callable[[str], None] = warn,
) -> int:
    root = package_root(source)
    problems = undeclared(
        source,
        manifest(f"{root}/pyproject.toml"),
        distributions(),
        read,
        files(source),
        ours(source),
    )
    for problem in problems:
        echo(f"undeclared dependency -- {problem}")
    if problems:
        echo(
            f"declared-deps: {len(problems)} import(s) not declared in {root}/pyproject.toml. "
            "Add each to [project].dependencies (or [dependency-groups].dev for a "
            "test-only import) and run `uv sync` -- never `uv pip install`, which "
            "populates the venv without declaring anything."
        )
        return 1
    return 0
