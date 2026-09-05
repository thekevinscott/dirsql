"""Colocated unit tests for the declared-deps command (isolation -- no `CliRunner`).

Driven through `.callback`; `run` (from `.run`) is mocked at its import site.
"""

from unittest import mock

import pytest

from checks.declared_deps.cli import cli


def invoke(**kwargs):
    with mock.patch("checks.declared_deps.cli.run", **kwargs) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(source="packages/python/dirsql")
    return run, exc_info.value.code


def test_exits_with_runs_return_code():
    run, code = invoke(return_value=0)
    run.assert_called_once_with("packages/python/dirsql")
    assert code == 0


def test_propagates_an_undeclared_import_as_exit_one():
    assert invoke(return_value=1)[1] == 1


def test_declares_a_required_source_argument():
    (argument,) = cli.params
    assert (argument.name, argument.required) == ("source", True)
