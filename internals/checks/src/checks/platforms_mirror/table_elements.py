"""The row expressions of a `platforms.py`-shaped module's `PLATFORMS` table.

Read statically off the module-level binding, so a table that builds itself at
import time is a `ParseError` rather than a read.
"""

from __future__ import annotations

import ast

from .assigned_names import assigned_names
from .parse import ParseError, TABLE_NAME


def table_elements(tree: ast.Module) -> list[ast.expr]:
    bindings = {name: node for node in tree.body for name in assigned_names(node)}
    assignment = bindings.get(TABLE_NAME)
    if assignment is None:
        raise ParseError(f"no module-level `{TABLE_NAME} = (...)` assignment.")
    if not isinstance(assignment.value, (ast.Tuple, ast.List)):
        raise ParseError(
            f"{TABLE_NAME} is not a tuple or list literal; this check reads its rows "
            f"statically and cannot evaluate a computed table."
        )
    return list(assignment.value.elts)
