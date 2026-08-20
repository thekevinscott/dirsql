"""The release-globs check -- repo-only (#944).

Backs `dirsql-checks release-globs`: the publish globs and the PR-time build
precheck must agree on which paths reach a released artifact.
"""

from __future__ import annotations

import click

from checks.release_globs.gate import run


@click.command()
@click.option(
    "--config",
    "config_path",
    default="putitoutthere.toml",
    help="Release config declaring each package's publish globs.",
)
@click.option(
    "--workflow",
    "workflow_path",
    default=".github/workflows/release-ci.yml",
    help="Workflow whose PR path filter gates the release build precheck.",
)
def cli(config_path: str, workflow_path: str) -> None:
    raise SystemExit(run(config_path, workflow_path))
