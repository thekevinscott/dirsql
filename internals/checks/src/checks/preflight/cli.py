"""The preflight check -- repo-only (#781).

Backs `dirsql-checks preflight`: derive the testing-conventions gate matrix from
`.github/workflows/conventions.yml` and run every pair locally.
"""

from __future__ import annotations

import click

from .gate import run


@click.command()
@click.option(
    "--conventions",
    default=".github/workflows/conventions.yml",
    help="Workflow whose reusable-workflow callers define the gate matrix.",
)
@click.option("--base", default="origin/main", help="Base ref the diff-scoped gates measure against.")
@click.option(
    "--gate",
    "gates",
    multiple=True,
    help="Run only these gates (repeatable). Default: every gate each root declares.",
)
@click.option("--dry-run", is_flag=True, help="Print the derived matrix without running it.")
def cli(conventions: str, base: str, gates: tuple[str, ...], dry_run: bool) -> None:
    with open(conventions, encoding="utf-8") as handle:
        text = handle.read()
    raise SystemExit(run(text, base, only=gates, dry_run=dry_run, echo=click.echo))
