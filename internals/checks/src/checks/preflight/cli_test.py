"""Colocated unit tests for the preflight command (isolation -- no `CliRunner`).

Driven through `.callback`; `run`, `glob` and `open` are mocked at their import site.
"""

from unittest import mock

import pytest

from checks.preflight.cli import cli

WORKFLOWS = [".github/workflows/b-ci.yml", ".github/workflows/a-ci.yml"]


def invoke(gates=(), dry_run=False, found=WORKFLOWS, **kwargs):
    opened = mock.mock_open(read_data="jobs: {}")
    with mock.patch("checks.preflight.cli.open", opened):
        with mock.patch("checks.preflight.cli.glob", return_value=list(found)) as globbed:
            with mock.patch("checks.preflight.cli.run", **kwargs) as run:
                with pytest.raises(SystemExit) as exc_info:
                    cli.callback(workflows="wf-dir", base="origin/x", gates=gates, dry_run=dry_run)
    return globbed, opened, run, exc_info.value.code


def test_globs_the_workflow_directory_for_yaml_files():
    globbed, _opened, _run, _code = invoke(return_value=0)
    globbed.assert_called_once_with("wf-dir/*.yml")


def test_reads_every_workflow_and_exits_with_runs_return_code():
    _globbed, opened, run, code = invoke(return_value=0)
    assert opened.call_args_list == [
        mock.call(".github/workflows/a-ci.yml", encoding="utf-8"),
        mock.call(".github/workflows/b-ci.yml", encoding="utf-8"),
    ]
    assert run.call_args.args == (["jobs: {}", "jobs: {}"], "origin/x")
    assert code == 0


def test_reads_the_workflows_in_sorted_order():
    # Glob order is filesystem-dependent; the matrix (and so the report) must not be.
    _globbed, opened, _run, _code = invoke(return_value=0)
    assert [call.args[0] for call in opened.call_args_list] == sorted(WORKFLOWS)


def test_propagates_a_failing_matrix_as_exit_one():
    assert invoke(return_value=1)[3] == 1


def test_forwards_the_gate_filter_and_dry_run_flag():
    _globbed, _opened, run, _code = invoke(
        gates=("unit-lint",), dry_run=True, return_value=0
    )
    assert run.call_args.kwargs["only"] == ("unit-lint",)
    assert run.call_args.kwargs["dry_run"] is True


def test_defaults_to_the_workflow_directory_and_main():
    workflows, base, gates, dry_run = cli.params
    assert workflows.default == ".github/workflows"
    assert base.default == "origin/main"
    assert gates.multiple is True
    assert dry_run.is_flag is True
