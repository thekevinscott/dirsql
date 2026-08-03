"""Colocated unit tests for the wheel-extension-load command (isolation -- no
`CliRunner`). Driven through `.callback`; `run` is mocked at its import site.
"""

from unittest import mock

import pytest

from checks.wheel_extension_load.cli import cli
from checks.wheel_extension_load.gate import ProbeError


def test_exits_with_runs_return_code():
    with mock.patch("checks.wheel_extension_load.cli.run", return_value=0) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(dist_dir="dist/")
        run.assert_called_once_with("dist/")
        assert exc_info.value.code == 0


def test_probe_error_prints_diagnostic_and_exits_one(capsys):
    with mock.patch(
        "checks.wheel_extension_load.cli.run",
        side_effect=ProbeError("static-pie cannot dlopen"),
    ):
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(dist_dir="dist/")
        assert exc_info.value.code == 1
    err = capsys.readouterr().err
    assert "wheel-extension-load: static-pie cannot dlopen" in err


def test_declares_a_required_dist_dir_option():
    (option,) = cli.params
    assert option.name == "dist_dir"
    assert option.required is True
