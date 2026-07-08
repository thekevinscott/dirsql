"""The changelog-gate check -- repo-only (#494/#496).

Backs the `dirsql-checks changelog-gate` subcommand: requires a CHANGELOG.md entry for PRs that
touch SDK code, with a `skip-changelog:` trailer escape hatch. Reads BASE_SHA/HEAD_SHA from the
environment (set by `.github/workflows/changelog-check.yml` from the PR's base/head SHAs) so the
workflow step stays a one-liner.
"""
from __future__ import annotations

import click

from checks.changelog_gate.gate import run


@click.command()
@click.option("--base-sha", envvar="BASE_SHA", required=True)
@click.option("--head-sha", envvar="HEAD_SHA", required=True)
def cli(base_sha: str, head_sha: str) -> None:
    raise SystemExit(run(base_sha, head_sha))
