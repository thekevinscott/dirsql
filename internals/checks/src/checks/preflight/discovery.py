"""Find the workflows in a directory that hold reusable-workflow callers (#781)."""

from __future__ import annotations

from collections.abc import Callable

from .matrix import NoGateMatrix, REUSABLE, parse_gate_matrix


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
