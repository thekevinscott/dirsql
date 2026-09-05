"""The `Platform` fields and `PLATFORMS` rows of a `platforms.py`-shaped module.

The field list comes off the dataclass annotations and the rows off the
module-level assignment, both through `ast`: the module is never imported, so a
table that builds itself at import time is a `ParseError` rather than a read.
"""

from __future__ import annotations

import ast

from .parse import CLASS_NAME, ParseError
from .row import row
from .table_elements import table_elements


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


def python_table(source: str) -> tuple[list[str], list[dict]]:
    """`(field names, rows)` from a `platforms.py`-shaped module."""
    tree = ast.parse(source)
    fields = _dataclass_fields(tree)
    return fields, [row(element, fields) for element in table_elements(tree)]
