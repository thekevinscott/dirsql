"""The artifact-completeness check -- repo-only (#790).

Backs `dirsql-checks artifact-completeness`: every (package, target) the
release config declares must have produced a non-empty build artifact.
"""

from __future__ import annotations

import click

from .gate import run


@click.command()
@click.option(
    "--dist-dir",
    required=True,
    help="Directory the run's artifacts were downloaded into, one subdirectory per artifact.",
)
@click.option(
    "--config",
    "config_path",
    default="putitoutthere.toml",
    help="Release config declaring the packages and their targets.",
)
def cli(dist_dir: str, config_path: str) -> None:
    raise SystemExit(run(dist_dir, config_path))
