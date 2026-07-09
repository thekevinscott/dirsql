"""The `dirsql-distcheck node` subcommand -- repo-only (#520).

Runs the npm packaging distcheck flow (build -> pack -> install -> run) over the
checkout, targeting the host platform. `--ts-pkg` defaults to this checkout's
`packages/ts` and exists so a manual run or the integration tier can point the
flow elsewhere. A `DistcheckError` from any stage becomes a non-zero exit.
"""
from __future__ import annotations

import os
import platform
import sys

import click

from distcheck.node_flow.gate import DistcheckError, run
from distcheck.node_flow.platforms import detect_host

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..", "..")
)
_TS_PKG = os.path.join(_REPO_ROOT, "packages", "ts")


@click.command()
@click.option("--ts-pkg", default=_TS_PKG, help="The packages/ts package directory.")
def cli(ts_pkg: str) -> None:
    host = detect_host(sys.platform, platform.machine())
    try:
        code = run(ts_pkg, host)
    except DistcheckError as err:
        raise SystemExit(str(err))
    click.echo("node packaging distcheck: OK")
    raise SystemExit(code)
