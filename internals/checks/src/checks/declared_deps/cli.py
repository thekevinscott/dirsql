"""The declared-deps check -- repo-only (#782).

Backs `dirsql-checks declared-deps <source>`: every third-party import in the
scanned tree must be declared in the owning package's pyproject.toml.
"""

from __future__ import annotations

import click

from .gate import run


@click.command()
@click.argument("source")
def cli(source: str) -> None:
    raise SystemExit(run(source))
