"""The wheel-extension-load check -- repo-only (#755).

Backs the `dirsql-checks wheel-extension-load` subcommand: probes that the
bundled binary inside a built wheel can load a real SQLite extension.
"""

from __future__ import annotations

import click

from checks.wheel_extension_load.gate import ProbeError, run


@click.command()
@click.option(
    "--dist-dir",
    required=True,
    help="Directory containing the built wheel to probe (may be empty/absent).",
)
def cli(dist_dir: str) -> None:
    try:
        code = run(dist_dir)
    except ProbeError as err:
        click.echo(f"wheel-extension-load: {err}", err=True)
        raise SystemExit(1) from err
    raise SystemExit(code)
