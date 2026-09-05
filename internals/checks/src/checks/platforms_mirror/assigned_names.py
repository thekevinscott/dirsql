"""The module-level names a statement binds."""

from __future__ import annotations

import ast


def assigned_names(node: ast.stmt) -> list[str]:
    """The module-level names ``node`` binds, empty when it binds none."""
    if isinstance(node, ast.AnnAssign):
        targets = [node.target]
    elif isinstance(node, ast.Assign):
        targets = node.targets
    else:
        return []
    return [target.id for target in targets if isinstance(target, ast.Name)]
