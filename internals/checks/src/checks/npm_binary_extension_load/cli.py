"""The npm-binary-extension-load check -- repo-only (#762).

Backs the `dirsql-checks npm-binary-extension-load` subcommand: probes
that the npm bundled-cli binary artifact can load a real SQLite
extension.
"""

from __future__ import annotations

import click

from checks.npm_binary_extension_load.gate import ProbeError, run


@click.command()
@click.option(
    "--dist-dir",
    required=True,
    help="Directory containing the downloaded bundled-cli binary to probe (may be empty/absent).",
)
def cli(dist_dir: str) -> None:
    try:
        code = run(dist_dir)
    except ProbeError as err:
        click.echo(f"npm-binary-extension-load: {err}", err=True)
        raise SystemExit(1) from err
    raise SystemExit(code)
