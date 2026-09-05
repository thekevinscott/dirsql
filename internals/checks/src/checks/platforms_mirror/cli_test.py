"""Colocated unit tests for the platforms-mirror command (isolation -- no
`CliRunner`). Driven through `.callback`; `run` is mocked at its import site.
"""

from unittest import mock

import pytest

from checks.platforms_mirror.cli import PYTHON_FILE, TYPESCRIPT_FILE, cli


def invoke(**kwargs):
    with mock.patch("checks.platforms_mirror.cli.run", **kwargs) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(python_path="p.py", typescript_path="t.ts")
    return run, exc_info.value.code


def test_exits_with_runs_return_code():
    run, code = invoke(return_value=0)
    run.assert_called_once_with("p.py", "t.ts")
    assert code == 0


def test_propagates_a_drifted_mirror_as_exit_one():
    assert invoke(return_value=1)[1] == 1


def test_defaults_to_the_two_committed_platform_tables():
    python_path, typescript_path = cli.params
    assert python_path.default == PYTHON_FILE
    assert typescript_path.default == TYPESCRIPT_FILE
    assert PYTHON_FILE == "internals/distcheck/src/distcheck/node_flow/platforms.py"
    assert TYPESCRIPT_FILE == "packages/ts/src/platforms.ts"
