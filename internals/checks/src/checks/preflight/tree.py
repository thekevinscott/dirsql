"""What the diff-scope warning needs to know about the working tree.

The gates that take `--base` compute their subject from the committed
`base...HEAD` range, so uncommitted edits are invisible to them however dirty
the tree is.
"""

from __future__ import annotations

import subprocess
from dataclasses import dataclass


@dataclass
class Tree:
    base: str
    dirty: bool
    committed: bool


def detect_tree(base: str) -> Tree:
    status = subprocess.run(
        ["git", "status", "--porcelain"], capture_output=True, text=True, check=False
    )
    diff = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...HEAD"], capture_output=True, text=True, check=False
    )
    return Tree(
        base=base,
        dirty=bool(status.stdout.strip()),
        # An unresolvable base leaves stdout empty too; report it as committed so
        # the warning claims nothing the probe could not see.
        committed=diff.returncode != 0 or bool(diff.stdout.strip()),
    )
