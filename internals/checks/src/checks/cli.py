"""The single console entry point: the click group that composes the checks (#494/#495).

Repo-only. `dirsql-checks` is one command; each check is a `@click.command()` in its own
subfolder, registered here as a subcommand. Adding a check is a folder plus one `add_command` line.
"""

from __future__ import annotations

import click

from checks.changelog_gate.cli import cli as changelog_gate
from checks.npm_binary_extension_load.cli import cli as npm_binary_extension_load
from checks.pytest_gate.cli import cli as pytest_gate
from checks.wheel_extension_load.cli import cli as wheel_extension_load


@click.group()
def main() -> None:
    """Repo-only CI helper checks for dirsql."""


main.add_command(changelog_gate, name="changelog-gate")
main.add_command(npm_binary_extension_load, name="npm-binary-extension-load")
main.add_command(pytest_gate, name="pytest-gate")
main.add_command(wheel_extension_load, name="wheel-extension-load")
