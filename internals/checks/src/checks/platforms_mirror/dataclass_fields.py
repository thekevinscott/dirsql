"""The mirrored field list, read off the `Platform` dataclass annotations."""

from __future__ import annotations

import ast

from .parse import CLASS_NAME, ParseError


def dataclass_fields(tree: ast.Module) -> list[str]:
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
