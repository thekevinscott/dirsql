"""The pytest-gate check — repo-only (#494/#495).

Backs the `dirsql-checks pytest-gate` subcommand: runs pytest over PATHS, translating pytest's
"no tests collected" exit code (5) to success so a directory without `*_test.py` files yet passes
cleanly, since pytest's own recursive collection would otherwise fail CI.

All arguments are passed through unprocessed to pytest (paths, `-x`, `--cov=...`, etc.) — this
command is a translating proxy, not a parser of pytest's own flags.
"""
from __future__ import annotations

import click

from checks.pytest_gate.gate import run


@click.command(context_settings={"ignore_unknown_options": True})
@click.argument("args", nargs=-1, type=click.UNPROCESSED)
def cli(args: tuple[str, ...]) -> None:
    raise SystemExit(run(list(args)))
