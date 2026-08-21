"""Reading the two platform tables as source text (#1004).

Neither side is imported: `internals/checks` depends on neither
`internals/distcheck` nor a node toolchain, and a check that executes the file
it audits can be fooled by the file. The Python table comes out of `ast`; the
TypeScript one out of a scan that strips comments and hands the remaining
object literal to `json`.

Every shape this cannot read raises `ParseError` rather than returning an empty
table -- a mirror check that silently sees no rows passes forever.
"""

from __future__ import annotations

import ast
import json
import re

CLASS_NAME = "Platform"
TABLE_NAME = "PLATFORMS"


class ParseError(Exception):
    """A platform table that could not be read in the shape this check expects."""


def _dataclass_fields(tree: ast.Module) -> list[str]:
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == CLASS_NAME:
            fields = [
                statement.target.id
                for statement in node.body
                if isinstance(statement, ast.AnnAssign)
                and isinstance(statement.target, ast.Name)
            ]
            if not fields:
                raise ParseError(
                    f"class {CLASS_NAME} declares no annotated fields; this check reads "
                    f"the mirrored field list off them."
                )
            return fields
    raise ParseError(
        f"no `class {CLASS_NAME}` at module level; this check reads the mirrored "
        f"field list off its annotations."
    )


def _table_elements(tree: ast.Module) -> list[ast.expr]:
    for node in tree.body:
        target = None
        if isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            target = node.target.id
        elif isinstance(node, ast.Assign) and len(node.targets) == 1:
            first = node.targets[0]
            target = first.id if isinstance(first, ast.Name) else None
        if target == TABLE_NAME:
            if not isinstance(node.value, (ast.Tuple, ast.List)):
                raise ParseError(
                    f"{TABLE_NAME} is not a tuple or list literal; this check reads its "
                    f"rows statically and cannot evaluate a computed table."
                )
            return list(node.value.elts)
    raise ParseError(f"no module-level `{TABLE_NAME} = (...)` assignment.")


def _row(element: ast.expr, fields: list[str]) -> dict:
    if not (
        isinstance(element, ast.Call)
        and isinstance(element.func, ast.Name)
        and element.func.id == CLASS_NAME
    ):
        raise ParseError(
            f"every {TABLE_NAME} entry must be a literal `{CLASS_NAME}(...)` call."
        )
    if len(element.args) > len(fields):
        raise ParseError(
            f"a {CLASS_NAME}(...) row passes {len(element.args)} positional arguments "
            f"but {CLASS_NAME} declares {len(fields)} fields."
        )
    row = dict(zip(fields, (_literal(argument) for argument in element.args)))
    for keyword in element.keywords:
        if keyword.arg is None:
            raise ParseError(f"a {CLASS_NAME}(...) row splats **kwargs; read it statically.")
        row[keyword.arg] = _literal(keyword.value)
    return row


def _literal(node: ast.expr):
    try:
        return ast.literal_eval(node)
    except ValueError as error:
        raise ParseError(
            f"a {CLASS_NAME}(...) argument is not a literal: {error}"
        ) from error


def python_table(source: str) -> tuple[list[str], list[dict]]:
    """`(field names, rows)` from a `platforms.py`-shaped module."""
    tree = ast.parse(source)
    fields = _dataclass_fields(tree)
    return fields, [_row(element, fields) for element in _table_elements(tree)]


def _literal_text(source: str, name: str) -> str:
    """The bracketed literal assigned to ``name``, comments stripped.

    Scanned rather than matched: the array spans lines, holds nested objects,
    and carries `//` comments that a regex would either keep or cut mid-string.
    """
    marker = re.search(rf"\b{name}\b[^=;]*=\s*\[", source)
    if marker is None:
        raise ParseError(f"no `{name} = [...]` assignment in the TypeScript source.")
    index = marker.end() - 1
    depth = 0
    out: list[str] = []
    while index < len(source):
        character = source[index]
        if character in "\"'`":
            index = _copy_string(source, index, out)
            continue
        if source.startswith("//", index):
            newline = source.find("\n", index)
            if newline == -1:
                break
            index = newline
            continue
        if source.startswith("/*", index):
            end = source.find("*/", index)
            if end == -1:
                raise ParseError(f"unterminated block comment inside `{name}`.")
            index = end + 2
            continue
        if character in "[{":
            depth += 1
        elif character in "]}":
            depth -= 1
        out.append(character)
        index += 1
        if depth == 0:
            return "".join(out)
    raise ParseError(f"unbalanced brackets in `{name}`.")


def _copy_string(source: str, index: int, out: list[str]) -> int:
    quote = source[index]
    out.append('"')
    index += 1
    while index < len(source) and source[index] != quote:
        if source[index] == "\\":
            out.append(source[index])
            index += 1
            if index >= len(source):
                break
        out.append(source[index])
        index += 1
    if index >= len(source):
        raise ParseError("unterminated string literal in the TypeScript source.")
    out.append('"')
    return index + 1


def typescript_table(source: str) -> list[dict]:
    """Rows of the `PLATFORMS` array in a `platforms.ts`-shaped module."""
    literal = _literal_text(source, TABLE_NAME)
    keyed = re.sub(r"([{,]\s*)([A-Za-z_$][\w$]*)\s*:", r'\1"\2":', literal)
    without_trailing_commas = re.sub(r",(\s*[}\]])", r"\1", keyed)
    try:
        rows = json.loads(without_trailing_commas)
    except json.JSONDecodeError as error:
        raise ParseError(
            f"`{TABLE_NAME}` is not a plain array of object literals ({error}); this "
            f"check reads it as data and cannot evaluate spreads, computed keys or "
            f"identifiers."
        ) from error
    if not all(isinstance(row, dict) for row in rows):
        raise ParseError(f"every `{TABLE_NAME}` entry must be an object literal.")
    return rows
