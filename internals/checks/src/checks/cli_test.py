"""Colocated unit test for the group: each check is registered as a subcommand (isolation).

Reads the composed group's command table -- no file I/O, no dispatch.
"""

from checks.cli import main


def test_artifact_completeness_is_registered():
    assert "artifact-completeness" in main.commands


def test_attestation_guard_is_registered():
    assert "attestation-guard" in main.commands


def test_changelog_gate_is_registered():
    assert "changelog-gate" in main.commands


def test_pytest_gate_is_registered():
    assert "pytest-gate" in main.commands


def test_wheel_extension_load_is_registered():
    assert "wheel-extension-load" in main.commands


def test_declared_deps_is_registered():
    assert "declared-deps" in main.commands


def test_preflight_is_registered():
    assert "preflight" in main.commands


def test_release_globs_is_registered():
    assert "release-globs" in main.commands
