"""The platforms-mirror check -- repo-only (#1004).

Backs `dirsql-checks platforms-mirror`: the node distcheck flow's platform table
and the TypeScript release source of truth must agree on the fields they share.
"""

from __future__ import annotations

import click

from checks.platforms_mirror.vocabulary import PYTHON_FILE, TYPESCRIPT_FILE
from checks.platforms_mirror.gate import run


@click.command()
@click.option(
    "--python",
    "python_path",
    default=PYTHON_FILE,
    help="The node distcheck flow's platform table.",
)
@click.option(
    "--typescript",
    "typescript_path",
    default=TYPESCRIPT_FILE,
    help="The release source of truth for published sub-packages.",
)
def cli(python_path: str, typescript_path: str) -> None:
    raise SystemExit(run(python_path, typescript_path))
