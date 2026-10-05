"""Colocated unit tests for the npm-addon-version command (isolation -- no
`CliRunner`). Driven through `.callback`; `run` is mocked at its import site.
"""

from unittest import mock

import pytest

from checks.npm_addon_version.cli import ProbeError, cli


def test_exits_with_runs_return_code():
    with mock.patch("checks.npm_addon_version.cli.run", return_value=0) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(dist_dir="dist/", manifest="Cargo.toml")
        run.assert_called_once_with("dist/", "Cargo.toml")
        assert exc_info.value.code == 0


def test_probe_error_prints_diagnostic_and_exits_one(capsys):
    with mock.patch(
        "checks.npm_addon_version.cli.run",
        side_effect=ProbeError("unstamped"),
    ):
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(dist_dir="dist/", manifest="Cargo.toml")
        assert exc_info.value.code == 1
    assert "npm-addon-version: unstamped" in capsys.readouterr().err


def test_declares_a_required_dist_dir_and_a_defaulted_manifest():
    dist_dir, manifest = cli.params
    assert dist_dir.name == "dist_dir"
    assert dist_dir.required is True
    assert manifest.name == "manifest"
    assert manifest.default == "packages/rust/Cargo.toml"
