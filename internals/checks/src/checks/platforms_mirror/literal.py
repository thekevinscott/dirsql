"""One `Platform(...)` argument, read rather than evaluated.

A row is only as trustworthy as its arguments: anything `ast` cannot constant-
fold is a `ParseError`, not a value the check would go on comparing.
"""

from __future__ import annotations

import ast

from .parse import CLASS_NAME, ParseError


def literal(node: ast.expr):
    try:
        return ast.literal_eval(node)
    except ValueError as error:
        raise ParseError(f"a {CLASS_NAME}(...) argument is not a literal: {error}") from error
