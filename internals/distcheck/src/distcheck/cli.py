"""The single console entry point: the click group that composes the distcheck flows (#520).

Repo-only. `dirsql-distcheck` is one command; each packaging distcheck flow is a
`@click.command()` in its own subfolder, registered here as a subcommand
(`dirsql-distcheck python`, `dirsql-distcheck node`). Adding a flow is a folder plus one
`add_command` line.
"""
from __future__ import annotations

import click

from distcheck.node_flow.cli import cli as node_flow
from distcheck.python_flow.cli import cli as python_flow


@click.group()
def main() -> None:
    """Repo-only packaging distcheck flows for dirsql."""


main.add_command(python_flow, name="python")
main.add_command(node_flow, name="node")
