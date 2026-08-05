"""Colocated unit tests for the preflight command (isolation -- no `CliRunner`).

Driven through `.callback`; `run` and `open` are mocked at their import site.
"""

from unittest import mock

import pytest

from checks.preflight.cli import cli


def invoke(gates=(), dry_run=False, **kwargs):
    with mock.patch("checks.preflight.cli.open", mock.mock_open(read_data="jobs: {}")) as opened:
        with mock.patch("checks.preflight.cli.run", **kwargs) as run:
            with pytest.raises(SystemExit) as exc_info:
                cli.callback(conventions="wf.yml", base="origin/x", gates=gates, dry_run=dry_run)
    return opened, run, exc_info.value.code


def test_reads_the_workflow_and_exits_with_runs_return_code():
    opened, run, code = invoke(return_value=0)
    opened.assert_called_once_with("wf.yml", encoding="utf-8")
    assert run.call_args.args == ("jobs: {}", "origin/x")
    assert code == 0


def test_propagates_a_failing_matrix_as_exit_one():
    assert invoke(return_value=1)[2] == 1


def test_forwards_the_gate_filter_and_dry_run_flag():
    _opened, run, _code = invoke(gates=("unit-lint",), dry_run=True, return_value=0)
    assert run.call_args.kwargs["only"] == ("unit-lint",)
    assert run.call_args.kwargs["dry_run"] is True


def test_defaults_to_the_ci_workflow_and_main():
    conventions, base, gates, dry_run = cli.params
    assert conventions.default == ".github/workflows/conventions.yml"
    assert base.default == "origin/main"
    assert gates.multiple is True
    assert dry_run.is_flag is True
