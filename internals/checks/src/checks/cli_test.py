"""Colocated unit test for the group: each check is registered as a subcommand (isolation).

Reads the composed group's command table -- no file I/O, no dispatch.
"""

from checks.cli import main


def test_changelog_gate_is_registered():
    assert "changelog-gate" in main.commands


def test_pytest_gate_is_registered():
    assert "pytest-gate" in main.commands


def test_wheel_extension_load_is_registered():
    assert "wheel-extension-load" in main.commands
