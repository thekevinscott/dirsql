"""Colocated unit tests for the release-globs command (isolation -- no
`CliRunner`). Driven through `.callback`; `run` is mocked at its import site.
"""

from unittest import mock

import pytest

from checks.release_globs.cli import cli


def invoke(**kwargs):
    with mock.patch("checks.release_globs.cli.run", **kwargs) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(config_path="p.toml", workflow_path="w.yml")
    return run, exc_info.value.code


def test_exits_with_runs_return_code():
    run, code = invoke(return_value=0)
    run.assert_called_once_with("p.toml", "w.yml")
    assert code == 0


def test_propagates_a_disagreeing_config_as_exit_one():
    assert invoke(return_value=1)[1] == 1


def test_defaults_to_the_repo_root_config_and_the_release_precheck_workflow():
    config_path, workflow_path = cli.params
    assert config_path.default == "putitoutthere.toml"
    assert workflow_path.default == ".github/workflows/release-ci.yml"
