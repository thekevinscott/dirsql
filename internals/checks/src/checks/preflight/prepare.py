"""The per-python-root drift guards that run before any gate (#782).

A `uv pip install` leaves the local venv strictly more capable than any real
install, and no amount of running more gates catches that.
"""

from __future__ import annotations

from collections.abc import Callable

from .invocation import Invocation
from .matrix import Root
from .package_root import package_root


def prepare(roots: list[Root], exists: Callable[[str], bool]) -> list[tuple[str, str, Invocation]]:
    """Per-python-root steps that guard against venv drift (#782).

    `uv sync` reconciles the venv with the manifest, *removing* anything a
    `uv pip install` left behind, so an undeclared dependency stops resolving
    locally the way it never resolved in CI. `declared-deps` is the direct
    assertion, independent of whatever the venv happens to hold.
    """
    steps = []
    for root in roots:
        if "python" not in root.languages:
            continue
        home = package_root(root.source, exists)
        steps.append((root.job, "uv-sync", Invocation(["uv", "sync", "--project", home], ".")))
        steps.append(
            (
                root.job,
                "declared-deps",
                Invocation(
                    [
                        *["uv", "run", "--project", "internals/checks", "dirsql-checks"],
                        *["declared-deps", root.source],
                    ],
                    ".",
                ),
            )
        )
    return steps
