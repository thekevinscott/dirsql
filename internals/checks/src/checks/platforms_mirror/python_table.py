"""The `Platform` fields and `PLATFORMS` rows of a `platforms.py`-shaped module.

The field list comes off the dataclass annotations and the rows off the
module-level assignment, both through `ast`: the module is never imported, so a
table that builds itself at import time is a `ParseError` rather than a read.
"""

from __future__ import annotations

import ast

from .parse import CLASS_NAME, ParseError, TABLE_NAME
from .row import row


def _dataclass_fields(tree: ast.Module) -> list[str]:
    classes = {node.name: node for node in tree.body if isinstance(node, ast.ClassDef)}
    declaration = classes.get(CLASS_NAME)
    if declaration is None:
        raise ParseError(
            f"no `class {CLASS_NAME}` at module level; this check reads the mirrored "
            f"field list off its annotations."
        )
    fields = [
        statement.target.id
        for statement in declaration.body
        if isinstance(statement, ast.AnnAssign) and isinstance(statement.target, ast.Name)
    ]
    if not fields:
        raise ParseError(
            f"class {CLASS_NAME} declares no annotated fields; this check reads the "
            f"mirrored field list off them."
        )
    return fields


def _assigned_names(node: ast.stmt) -> list[str]:
    """The module-level names ``node`` binds, empty when it binds none."""
    if isinstance(node, ast.AnnAssign):
        targets = [node.target]
    elif isinstance(node, ast.Assign):
        targets = node.targets
    else:
        return []
    return [target.id for target in targets if isinstance(target, ast.Name)]


def _table_elements(tree: ast.Module) -> list[ast.expr]:
    bindings = {name: node for node in tree.body for name in _assigned_names(node)}
    assignment = bindings.get(TABLE_NAME)
    if assignment is None:
        raise ParseError(f"no module-level `{TABLE_NAME} = (...)` assignment.")
    if not isinstance(assignment.value, (ast.Tuple, ast.List)):
        raise ParseError(
            f"{TABLE_NAME} is not a tuple or list literal; this check reads its rows "
            f"statically and cannot evaluate a computed table."
        )
    return list(assignment.value.elts)


def python_table(source: str) -> tuple[list[str], list[dict]]:
    """`(field names, rows)` from a `platforms.py`-shaped module."""
    tree = ast.parse(source)
    fields = _dataclass_fields(tree)
    return fields, [row(element, fields) for element in _table_elements(tree)]
