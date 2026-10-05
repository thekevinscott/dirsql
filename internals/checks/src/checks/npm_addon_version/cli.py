"""The npm-addon-version check -- repo-only.

Backs `dirsql-checks npm-addon-version`: probes that the release-built napi
addon's CLI reports the stamped release version, not the committed literal.
"""

from __future__ import annotations

import click

from checks.npm_addon_version.gate import ProbeError, run


@click.command()
@click.option(
    "--dist-dir",
    required=True,
    help="Directory containing the built napi addon to probe (may be empty/absent).",
)
@click.option(
    "--manifest",
    default="packages/rust/Cargo.toml",
    help="Manifest of the crate whose version the CLI prints.",
)
def cli(dist_dir: str, manifest: str) -> None:
    try:
        code = run(dist_dir, manifest)
    except ProbeError as err:
        click.echo(f"npm-addon-version: {err}", err=True)
        raise SystemExit(1) from err
    raise SystemExit(code)
