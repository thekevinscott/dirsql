"""The single console entry point: the click group that composes the checks (#494/#495).

Repo-only. `dirsql-checks` is one command; each check is a `@click.command()` in its own
subfolder, registered here as a subcommand. Adding a check is a folder plus one `add_command` line.
"""
from __future__ import annotations

import click

from checks.changelog_gate.cli import cli as changelog_gate
from checks.pytest_gate.cli import cli as pytest_gate


@click.group()
def main() -> None:
    """Repo-only CI helper checks for dirsql."""


main.add_command(changelog_gate, name="changelog-gate")
main.add_command(pytest_gate, name="pytest-gate")
