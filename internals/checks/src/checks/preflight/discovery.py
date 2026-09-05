"""Find the workflows the gate matrix is derived from (#781).

Which workflows hold the reusable-workflow callers is itself discovered rather
than named. The matrix used to come from `conventions.yml` alone; #834 split it
into six per-domain workflows and deleted it, so the named default resolved to
nothing and `just preflight` died on a bare `FileNotFoundError` (#973). A list of
six names would only move the same failure one rename away.
"""

from __future__ import annotations

import os
from collections.abc import Callable, Sequence

from .matrix import NoGateMatrix, REUSABLE, WORKFLOWS, parse_gate_matrix, read_text
from .named_workflow import named


def discovered(
    directory: str,
    listdir: Callable[[str], list[str]],
    read: Callable[[str], str],
) -> list[tuple[str, str]]:
    """(path, text) for every workflow in `directory` holding a caller."""
    try:
        names = sorted(listdir(directory))
    except OSError as err:
        raise NoGateMatrix(
            f"no {directory} directory here. Run preflight from the repo root, or "
            "name a workflow with --conventions."
        ) from err
    found = []
    for name in names:
        if not name.endswith((".yml", ".yaml")):
            continue
        path = f"{directory}/{name}"
        text = read(path)
        if parse_gate_matrix(text):
            found.append((path, text))
    if not found:
        raise NoGateMatrix(
            f"no workflow in {directory} calls {REUSABLE}, so there is no gate matrix "
            "to run. If the reusable workflow moved, update REUSABLE in "
            "internals/checks/src/checks/preflight/matrix.py; otherwise name the "
            "workflow with --conventions."
        )
    return found


def sources(
    conventions: Sequence[str],
    *,
    directory: str = WORKFLOWS,
    listdir: Callable[[str], list[str]] = os.listdir,
    read: Callable[[str], str] = read_text,
) -> list[tuple[str, str]]:
    """(path, text) for the workflows the gate matrix is derived from."""
    if conventions:
        return [(path, named(path, read)) for path in conventions]
    return discovered(directory, listdir, read)


