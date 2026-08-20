"""The preflight check -- repo-only (#781).

Backs `dirsql-checks preflight`: derive the testing-conventions gate matrix from
the CI workflows and run every pair locally.
"""

from __future__ import annotations

import os.path
from glob import glob

import click

from .gate import default_runner, read_e2e, run


@click.command()
@click.option(
    "--workflows",
    default=".github/workflows",
    help="Directory whose reusable-workflow callers define the gate matrix.",
)
@click.option("--base", default="origin/main", help="Base ref the diff-scoped gates measure against.")
@click.option(
    "--gate",
    "gates",
    multiple=True,
    help="Run only these gates (repeatable). Default: every gate each root declares.",
)
@click.option("--dry-run", is_flag=True, help="Print the derived matrix without running it.")
def cli(workflows: str, base: str, gates: tuple[str, ...], dry_run: bool) -> None:
    texts = []
    # Sorted, so the matrix -- and the report naming a failing pair -- does not
    # depend on the order the filesystem happens to hand back.
    for path in sorted(glob(f"{workflows}/*.yml")):
        with open(path, encoding="utf-8") as handle:
            texts.append(handle.read())
    raise SystemExit(
        run(
            texts,
            base,
            runner=default_runner,
            exists=os.path.exists,
            e2e_config=read_e2e,
            echo=click.echo,
            only=gates,
            dry_run=dry_run,
        )
    )
