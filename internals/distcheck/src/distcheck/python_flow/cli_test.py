"""Colocated unit tests for the `dirsql-distcheck python` command (isolation).

Driven through `.callback`; `run` is patched by string at its import site so no
build, subprocess, or filesystem collaborator runs. `DistcheckError` and the default
roots are read from the SUT module (which re-exports them), keeping the test's
imports to the unit under test alone.
"""
import os
from unittest import mock

import pytest

from distcheck.python_flow.cli import _PKG_ROOT, _REPO_ROOT, DistcheckError, cli


def test_runs_flow_and_exits_with_its_code(capsys):
    with mock.patch("distcheck.python_flow.cli.run", return_value=0) as run:
        with pytest.raises(SystemExit) as exc:
            cli.callback(repo_root="/repo", pkg_root="/pkg")
    run.assert_called_once_with("/pkg", "/repo")
    assert exc.value.code == 0
    assert "python packaging distcheck: OK" in capsys.readouterr().out


def test_distcheck_error_becomes_nonzero_exit_with_message(capsys):
    with mock.patch("distcheck.python_flow.cli.run", side_effect=DistcheckError("boom")):
        with pytest.raises(SystemExit) as exc:
            cli.callback(repo_root="/repo", pkg_root="/pkg")
    assert exc.value.code == "boom"
    assert "OK" not in capsys.readouterr().out


def test_default_roots_derive_from_this_checkout():
    assert os.path.isabs(_REPO_ROOT)
    assert ".." not in _REPO_ROOT.split(os.sep)
    assert _PKG_ROOT == os.path.join(_REPO_ROOT, "packages", "python")


def test_declares_repo_root_and_pkg_root_options():
    options = {p.name: p for p in cli.params}
    assert set(options) == {"repo_root", "pkg_root"}
    assert options["repo_root"].default == _REPO_ROOT
    assert options["pkg_root"].default == _PKG_ROOT
