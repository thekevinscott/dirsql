"""The `dirsql-distcheck python` subcommand -- repo-only (#520).

Runs the PyPI-wheel packaging distcheck flow (build -> pack -> install -> run) over
the checkout. `--repo-root` / `--pkg-root` default to this checkout's layout and
exist so a manual run or the integration tier can point the flow at an explicit
tree. A `DistcheckError` from any stage becomes a non-zero exit with its diagnostic.
"""
from __future__ import annotations

import os

import click

from distcheck.python_flow.gate import DistcheckError, run

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..", "..")
)
_PKG_ROOT = os.path.join(_REPO_ROOT, "packages", "python")


@click.command()
@click.option("--repo-root", default=_REPO_ROOT, help="Checkout root.")
@click.option(
    "--pkg-root", default=_PKG_ROOT, help="The packages/python package directory."
)
def cli(repo_root: str, pkg_root: str) -> None:
    try:
        code = run(pkg_root, repo_root)
    except DistcheckError as err:
        raise SystemExit(str(err))
    click.echo("python packaging distcheck: OK")
    raise SystemExit(code)
