"""The attestation-guard check -- repo-only (#1043).

Backs the `dirsql-checks attestation-guard` subcommand: fails a PR that deletes
an e2e attestation receipt. Reads BASE_SHA/HEAD_SHA from the environment (set by
`.github/workflows/attestation-guard.yml`) so the workflow step stays a one-liner.
"""

from __future__ import annotations

import click

from checks.attestation_guard.gate import run


@click.command()
@click.option("--base-sha", envvar="BASE_SHA", required=True)
@click.option("--head-sha", envvar="HEAD_SHA", required=True)
def cli(base_sha: str, head_sha: str) -> None:
    raise SystemExit(run(base_sha, head_sha))
