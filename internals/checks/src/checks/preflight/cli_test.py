"""Colocated unit tests for the preflight command (isolation -- no `CliRunner`).

Driven through `.callback`; `sources` and `run` are mocked at their import site.
"""

from unittest import mock

import pytest

from checks.preflight.cli import cli, run, sources

WORKFLOWS = [(".github/workflows/a-ci.yml", "jobs: {}"), (".github/workflows/b-ci.yml", "jobs: {}")]


class NoGateMatrix(Exception):
    """Stand-in for `matrix.NoGateMatrix` -- faked rather than imported.

    Patched over the name the `except` clause resolves at raise time, so the
    command's error path is exercised without importing the collaborator.
    """


def invoke(conventions=(), gates=(), dry_run=False, resolve=None, **kwargs):
    with (
        mock.patch("checks.preflight.cli.NoGateMatrix", NoGateMatrix),
        mock.patch(
            "checks.preflight.cli.sources", resolve or mock.Mock(return_value=WORKFLOWS)
        ) as sources,
        mock.patch("checks.preflight.cli.run", **kwargs) as run,
        mock.patch("checks.preflight.cli.click.echo") as echo,
    ):
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(
                conventions=conventions,
                base="origin/x",
                gates=gates,
                dry_run=dry_run,
            )
    return sources, run, echo, exc_info.value.code


def test_runs_every_resolved_workflow_and_exits_with_runs_return_code():
    _sources, run, _echo, code = invoke(return_value=0)
    assert run.call_args.args == (["jobs: {}", "jobs: {}"], "origin/x")
    assert code == 0


def test_passes_the_named_workflows_through_to_the_resolver():
    sources, _run, _echo, _code = invoke(conventions=("wf.yml",), return_value=0)
    assert sources.call_args.args == (("wf.yml",),)


def test_announces_which_workflows_the_matrix_came_from():
    _sources, _run, echo, _code = invoke(return_value=0)
    assert echo.call_args_list[0] == mock.call(
        "preflight: gate matrix from .github/workflows/a-ci.yml, .github/workflows/b-ci.yml"
    )


def test_propagates_a_failing_matrix_as_exit_one():
    assert invoke(return_value=1)[3] == 1


def test_reports_an_unresolvable_matrix_without_running_anything():
    # #973: a workflow that no longer exists has to say so, not raise.
    resolve = mock.Mock(side_effect=NoGateMatrix("--conventions gone.yml: no such workflow."))
    _sources, run, echo, code = invoke(resolve=resolve, return_value=0)
    assert (run.called, code) == (False, 1)
    assert echo.call_args == mock.call(
        "preflight: --conventions gone.yml: no such workflow.", err=True
    )


def test_forwards_the_gate_filter_and_dry_run_flag():
    _sources, run, _echo, _code = invoke(gates=("unit-lint",), dry_run=True, return_value=0)
    assert run.call_args.kwargs["only"] == ("unit-lint",)
    assert run.call_args.kwargs["dry_run"] is True


def test_defaults_to_discovering_the_workflows_and_main():
    # Parsed rather than read off the params: the effective default for
    # `--conventions` is what #973 turned into a crash, and an empty one is what
    # sends the command to discovery.
    with cli.make_context("preflight", []) as ctx:
        assert ctx.params == {
            "conventions": (),
            "base": "origin/main",
            "gates": (),
            "dry_run": False,
        }


def test_binds_the_resolver_and_the_runner_from_their_own_modules():
    assert (sources.__module__, run.__module__) == (
        "checks.preflight.sources",
        "checks.preflight.run",
    )
