"""The preflight check -- repo-only (#781).

Backs `dirsql-checks preflight`: derive the testing-conventions gate matrix from
the workflows in `.github/workflows/` that call the reusable workflow, and run
every pair locally.
"""

from __future__ import annotations

import os.path

import click

from .discovery import sources
from .gate import default_runner, read_e2e, run
from .matrix import NoGateMatrix, WORKFLOWS


@click.command()
@click.option(
    "--conventions",
    "conventions",
    multiple=True,
    help=(
        "Workflow whose reusable-workflow callers define the gate matrix (repeatable). "
        f"Default: every caller in {WORKFLOWS}."
    ),
)
@click.option("--base", default="origin/main", help="Base ref the diff-scoped gates measure against.")
@click.option(
    "--gate",
    "gates",
    multiple=True,
    help="Run only these gates (repeatable). Default: every gate each root declares.",
)
@click.option("--dry-run", is_flag=True, help="Print the derived matrix without running it.")
def cli(conventions: tuple[str, ...], base: str, gates: tuple[str, ...], dry_run: bool) -> None:
    try:
        workflows = sources(conventions)
    except NoGateMatrix as err:
        click.echo(f"preflight: {err}", err=True)
        raise SystemExit(1) from err
    click.echo(f"preflight: gate matrix from {', '.join(path for path, _text in workflows)}")
    raise SystemExit(
        run(
            [text for _path, text in workflows],
            base,
            runner=default_runner,
            exists=os.path.exists,
            e2e_config=read_e2e,
            echo=click.echo,
            only=gates,
            dry_run=dry_run,
        )
    )
