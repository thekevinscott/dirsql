"""Import extraction for the declared-deps check (#782)."""

from __future__ import annotations

import ast


def top_level_imports(text: str) -> set[str]:
    """Top-level module names imported by a source file; relative imports are ours."""
    names = set()
    for node in ast.walk(ast.parse(text)):
        if isinstance(node, ast.Import):
            names.update(alias.name.split(".")[0] for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and not node.level and node.module:
            names.add(node.module.split(".")[0])
    return names
