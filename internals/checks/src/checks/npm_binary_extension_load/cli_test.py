"""Colocated unit tests for the npm-binary-extension-load command (isolation --
no `CliRunner`). Driven through `.callback`; `run` is mocked at its import site.
"""

from unittest import mock

import pytest

from checks.npm_binary_extension_load.cli import ProbeError, cli


def test_exits_with_runs_return_code():
    with mock.patch(
        "checks.npm_binary_extension_load.cli.run", return_value=0
    ) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(dist_dir="npm-dist/")
        run.assert_called_once_with("npm-dist/")
        assert exc_info.value.code == 0


def test_probe_error_prints_diagnostic_and_exits_one(capsys):
    with mock.patch(
        "checks.npm_binary_extension_load.cli.run",
        side_effect=ProbeError("static-pie cannot dlopen"),
    ):
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(dist_dir="npm-dist/")
        assert exc_info.value.code == 1
    err = capsys.readouterr().err
    assert "npm-binary-extension-load: static-pie cannot dlopen" in err


def test_declares_a_required_dist_dir_option():
    (option,) = cli.params
    assert option.name == "dist_dir"
    assert option.required is True
