"""Name the diff-scoped gates that a dirty working tree hides work from."""

from __future__ import annotations

from collections.abc import Sequence

from .matrix import GATES
from .tree import Tree

TAIL = "preflight: commit first, then re-run."


def diff_scope_warning(tree: Tree, gates: Sequence[str]) -> list[str]:
    if not tree.dirty:
        return []
    scoped = list(dict.fromkeys(gate for gate in gates if GATES[gate].base))
    if not scoped:
        return []
    lead = (
        [
            "preflight: the working tree is dirty, so these gates measure the committed",
            "preflight: range only and will not see the uncommitted edits:",
        ]
        if tree.committed
        else [
            f"preflight: the working tree is dirty but nothing is committed against {tree.base}.",
            "preflight: these gates read the committed range, so they will examine nothing:",
        ]
    )
    return [*lead, f"preflight:   {', '.join(scoped)}", TAIL]
