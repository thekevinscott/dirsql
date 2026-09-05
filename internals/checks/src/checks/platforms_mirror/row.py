"""One `Platform(...)` entry of the Python table, as a dict (#1004).

Read statically rather than evaluated, so a row built by anything other than a
literal call -- a splat, a name, a computed value -- is a `ParseError` instead
of a row the check would go on comparing.
"""

from __future__ import annotations

import ast

from .call_name import call_name
from .literal import literal
from .parse import CLASS_NAME, ParseError, TABLE_NAME

ROW_CALLS = frozenset({CLASS_NAME})


def row(element: ast.expr, fields: list[str]) -> dict:
    if call_name(element) not in ROW_CALLS:
        raise ParseError(f"every {TABLE_NAME} entry must be a literal `{CLASS_NAME}(...)` call.")
    if len(element.args) > len(fields):
        raise ParseError(
            f"a {CLASS_NAME}(...) row passes {len(element.args)} positional arguments but "
            f"{CLASS_NAME} declares {len(fields)} fields."
        )
    values = dict(zip(fields, (literal(argument) for argument in element.args)))
    for keyword in element.keywords:
        if keyword.arg is None:
            raise ParseError(f"a {CLASS_NAME}(...) row splats **kwargs; read it statically.")
        values[keyword.arg] = literal(keyword.value)
    return values
