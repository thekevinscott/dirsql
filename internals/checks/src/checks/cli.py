"""The single console entry point: the click group that composes the checks (#494/#495).

Repo-only. `dirsql-checks` is one command; each check is a `@click.command()` in its own
subfolder, registered here as a subcommand. Adding a check is a folder plus one `add_command` line.
"""

from __future__ import annotations

import click

from checks.artifact_completeness.cli import cli as artifact_completeness
from checks.attestation_guard.cli import cli as attestation_guard
from checks.changelog_gate.cli import cli as changelog_gate
from checks.declared_deps.cli import cli as declared_deps
from checks.platforms_mirror.cli import cli as platforms_mirror
from checks.preflight.cli import cli as preflight
from checks.pytest_gate.cli import cli as pytest_gate
from checks.release_globs.cli import cli as release_globs
from checks.wheel_extension_load.cli import cli as wheel_extension_load


@click.group()
def main() -> None:
    """Repo-only CI helper checks for dirsql."""


main.add_command(artifact_completeness, name="artifact-completeness")
main.add_command(attestation_guard, name="attestation-guard")
main.add_command(changelog_gate, name="changelog-gate")
main.add_command(declared_deps, name="declared-deps")
main.add_command(platforms_mirror, name="platforms-mirror")
main.add_command(preflight, name="preflight")
main.add_command(pytest_gate, name="pytest-gate")
main.add_command(release_globs, name="release-globs")
main.add_command(wheel_extension_load, name="wheel-extension-load")
