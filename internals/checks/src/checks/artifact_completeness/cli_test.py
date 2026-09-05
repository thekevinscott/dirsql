"""Colocated unit tests for the artifact-completeness command (isolation --
no `CliRunner`). Driven through `.callback`; `run` (from `.run`) is
mocked at its import site.
"""

from unittest import mock

import pytest

from checks.artifact_completeness.cli import cli


def invoke(**kwargs):
    with mock.patch("checks.artifact_completeness.cli.run", **kwargs) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(dist_dir="dist/", config_path="p.toml")
    return run, exc_info.value.code


def test_exits_with_runs_return_code():
    run, code = invoke(return_value=0)
    run.assert_called_once_with("dist/", "p.toml")
    assert code == 0


def test_propagates_an_incomplete_matrix_as_exit_one():
    assert invoke(return_value=1)[1] == 1


def test_requires_a_dist_dir_and_defaults_the_config():
    dist_dir, config_path = cli.params
    assert (dist_dir.name, dist_dir.required) == ("dist_dir", True)
    assert config_path.default == "putitoutthere.toml"
