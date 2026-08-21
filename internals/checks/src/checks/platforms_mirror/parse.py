"""Reading the two platform tables as source text (#1004).

Neither side is imported: `internals/checks` depends on neither
`internals/distcheck` nor a node toolchain, and a check that executes the file
it audits can be fooled by the file. The Python table comes out of `ast`; the
TypeScript one is stripped of comments, normalized to JSON, and handed to
`json`'s own decoder, which finds the end of the array so nothing here counts
brackets.

Every shape this cannot read raises `ParseError` rather than returning an empty
table -- a mirror check that silently sees no rows passes forever.
"""

from __future__ import annotations

import ast
import json
import re

CLASS_NAME = "Platform"
TABLE_NAME = "PLATFORMS"
ROW_CALLS = frozenset({CLASS_NAME})

# One alternation of "things a naive scan would cut in half": the three string
# forms first, so a `//` inside a string is matched as string rather than as the
# comment that follows it.
_TOKENS = re.compile(
    r'"(?:\\.|[^"\\])*"' r"|'(?:\\.|[^'\\])*'" r"|`(?:\\.|[^`\\])*`" r"|//[^\n]*" r"|/\*.*?\*/",
    re.S,
)


class ParseError(Exception):
    """A platform table that could not be read in the shape this check expects."""


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


def _call_name(element: ast.expr):
    """The name of a plain `Name(...)` call, else ``None``."""
    if isinstance(element, ast.Call) and isinstance(element.func, ast.Name):
        return element.func.id
    return None


def _literal(node: ast.expr):
    try:
        return ast.literal_eval(node)
    except ValueError as error:
        raise ParseError(f"a {CLASS_NAME}(...) argument is not a literal: {error}") from error


def _row(element: ast.expr, fields: list[str]) -> dict:
    if _call_name(element) not in ROW_CALLS:
        raise ParseError(f"every {TABLE_NAME} entry must be a literal `{CLASS_NAME}(...)` call.")
    if len(element.args) > len(fields):
        raise ParseError(
            f"a {CLASS_NAME}(...) row passes {len(element.args)} positional arguments but "
            f"{CLASS_NAME} declares {len(fields)} fields."
        )
    row = dict(zip(fields, (_literal(argument) for argument in element.args)))
    for keyword in element.keywords:
        if keyword.arg is None:
            raise ParseError(f"a {CLASS_NAME}(...) row splats **kwargs; read it statically.")
        row[keyword.arg] = _literal(keyword.value)
    return row


def python_table(source: str) -> tuple[list[str], list[dict]]:
    """`(field names, rows)` from a `platforms.py`-shaped module."""
    tree = ast.parse(source)
    fields = _dataclass_fields(tree)
    return fields, [_row(element, fields) for element in _table_elements(tree)]


def _requoted(text: str) -> str:
    """A TypeScript string literal as a JSON one."""
    if text.startswith('"'):
        return text
    return json.dumps(text[1:-1].replace('\\"', '"').replace("\\'", "'"))


def _without_comments(source: str) -> str:
    def keep(match: re.Match) -> str:
        text = match.group(0)
        return "" if text.startswith(("//", "/*")) else _requoted(text)

    return _TOKENS.sub(keep, source)


def _as_json(text: str) -> str:
    keyed = re.sub(r"([{,]\s*)([A-Za-z_$][\w$]*)\s*:", r'\1"\2":', text)
    return re.sub(r",(\s*[}\]])", r"\1", keyed)


def typescript_table(source: str) -> list[dict]:
    """Rows of the `PLATFORMS` array in a `platforms.ts`-shaped module."""
    cleaned = _without_comments(source)
    marker = re.search(rf"\b{TABLE_NAME}\b[^=;]*=\s*(\[)", cleaned)
    if marker is None:
        raise ParseError(f"no `{TABLE_NAME} = [...]` assignment in the TypeScript source.")
    try:
        # `raw_decode` stops at the end of the array, so nothing here has to find
        # the closing bracket or care what follows it.
        rows, _ = json.JSONDecoder().raw_decode(_as_json(cleaned[marker.start(1) :]))
    except json.JSONDecodeError as error:
        raise ParseError(
            f"`{TABLE_NAME}` is not a plain array of object literals ({error}); this check "
            f"reads it as data and cannot evaluate spreads, computed keys or identifiers, "
            f"and cannot read an unterminated array, string or comment."
        ) from error
    if not all(isinstance(row, dict) for row in rows):
        raise ParseError(f"every `{TABLE_NAME}` entry must be an object literal.")
    return rows
