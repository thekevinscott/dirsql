"""Colocated unit tests for the pytest-gate command (isolation — no `CliRunner`).

Driven through `.callback` (the undecorated function); `run` (from `.run`) is mocked at its
import site in this module so no subprocess or filesystem collaborator runs during the test.
"""
from unittest import mock

import pytest

from checks.pytest_gate.cli import cli, run


def test_exits_with_runs_interpreted_return_code():
    with mock.patch("checks.pytest_gate.cli.run", return_value=0) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(args=("pkg/", "-x"))
        run.assert_called_once_with(["pkg/", "-x"])
        assert exc_info.value.code == 0


def test_propagates_a_nonzero_return_code():
    with mock.patch("checks.pytest_gate.cli.run", return_value=1) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(args=())
        run.assert_called_once_with([])
        assert exc_info.value.code == 1


def test_declares_a_variadic_args_argument():
    (argument,) = cli.params
    assert argument.name == "args"
    assert argument.nargs == -1


def test_binds_run_from_the_orchestration_module():
    assert run.__module__ == "checks.pytest_gate.run"
