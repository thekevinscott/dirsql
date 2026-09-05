"""Orchestration for the platforms-mirror check (#1004).

Reads both platform tables off disk and holds them to the subset invariant in
`vocabulary.py`. A table this cannot parse fails the check rather than passing
on an empty read: the whole point is that nothing silently stops comparing.
"""

from __future__ import annotations

from collections.abc import Callable

from .parse import ParseError
from .problems import problems
from .python_table import python_table
from .typescript_table import typescript_table


def read(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def run(
    python_path: str,
    typescript_path: str,
    source: Callable[[str], str] = read,
    echo: Callable[[str], None] = print,
) -> int:
    try:
        fields, python_rows = python_table(source(python_path))
        typescript_rows = typescript_table(source(typescript_path))
    except ParseError as error:
        echo(f"::error::platforms-mirror could not read a platform table: {error}")
        return 1
    found = problems(fields, python_rows, typescript_rows)
    for problem in found:
        echo(f"::error::{problem}")
    if found:
        echo(
            f"platforms-mirror: {len(found)} problem(s). platforms.ts is the release "
            f"source of truth for the published sub-packages; platforms.py mirrors the "
            f"subset named in "
            f"internals/checks/src/checks/platforms_mirror/vocabulary.py's SHARED."
        )
        return 1
    echo(
        f"ok platforms-mirror: {python_path} mirrors the shared fields of "
        f"{typescript_path} across {len(python_rows)} published target(s)."
    )
    return 0
