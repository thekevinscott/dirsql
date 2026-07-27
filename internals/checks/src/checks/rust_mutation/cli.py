"""The rust-mutation check -- repo-only (#672).

Backs the `dirsql-checks rust-mutation` subcommand: drives cargo-mutants over the PR's
changed Rust lines with a workspace-relative diff, working around testing-conventions'
crate-relative `--in-diff` that silently tests zero mutants for the workspace-member crate.
"""
from __future__ import annotations

import click

from checks.rust_mutation.gate import run


@click.command()
@click.option("--base", default="origin/main", show_default=True, help="Merge-base ref for the PR diff.")
def cli(base: str) -> None:
    raise SystemExit(run(base))
