"""Colocated unit tests for the `dirsql-distcheck node` command (isolation).

Driven through `.callback`; `run` and `detect_host` are patched by string at
their import site so no pack, install, or filesystem collaborator runs.
`DistcheckError` and the default root are read from the SUT module (which re-exports
/ defines them), keeping the test's imports to the unit under test alone.
"""
import os
from unittest import mock

import pytest

from distcheck.node_flow.cli import _REPO_ROOT, _TS_PKG, DistcheckError, cli


def test_detects_host_then_runs_flow_and_exits_with_its_code(capsys):
    with (
        mock.patch("distcheck.node_flow.cli.run", return_value=0) as run,
        mock.patch("distcheck.node_flow.cli.detect_host", return_value="HOST") as detect,
    ):
        with pytest.raises(SystemExit) as exc:
            cli.callback(ts_pkg="/ts")
    detect.assert_called_once()
    run.assert_called_once_with("/ts", "HOST")
    assert exc.value.code == 0
    assert "node packaging distcheck: OK" in capsys.readouterr().out


def test_distcheck_error_becomes_nonzero_exit_with_message(capsys):
    with (
        mock.patch("distcheck.node_flow.cli.run", side_effect=DistcheckError("boom")),
        mock.patch("distcheck.node_flow.cli.detect_host", return_value="HOST"),
    ):
        with pytest.raises(SystemExit) as exc:
            cli.callback(ts_pkg="/ts")
    assert exc.value.code == "boom"
    assert "OK" not in capsys.readouterr().out


def test_default_ts_pkg_derives_from_this_checkout():
    assert os.path.isabs(_REPO_ROOT)
    assert ".." not in _REPO_ROOT.split(os.sep)
    assert _TS_PKG == os.path.join(_REPO_ROOT, "packages", "ts")


def test_declares_ts_pkg_option():
    (option,) = cli.params
    assert option.name == "ts_pkg"
    assert option.default == _TS_PKG
