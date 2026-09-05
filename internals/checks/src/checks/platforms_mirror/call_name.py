"""The name of a table entry's call, when it has one."""

from __future__ import annotations

import ast


def call_name(element: ast.expr):
    """The name of a plain `Name(...)` call, else ``None``."""
    if isinstance(element, ast.Call) and isinstance(element.func, ast.Name):
        return element.func.id
    return None
