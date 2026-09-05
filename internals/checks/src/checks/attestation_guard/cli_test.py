"""Colocated unit tests for the attestation-guard command (isolation -- no `CliRunner`).

Driven through `.callback`; `run` is mocked at its import site in this module.
"""

from unittest import mock

import pytest

from checks.attestation_guard.cli import cli


def test_exits_with_runs_return_code():
    with mock.patch("checks.attestation_guard.cli.run", return_value=0) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(base_sha="abc", head_sha="def")
        run.assert_called_once_with("abc", "def")
        assert exc_info.value.code == 0


def test_propagates_a_nonzero_return_code():
    with mock.patch("checks.attestation_guard.cli.run", return_value=1) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(base_sha="abc", head_sha="def")
        run.assert_called_once_with("abc", "def")
        assert exc_info.value.code == 1


def test_declares_base_sha_and_head_sha_options_reading_the_matching_envvars():
    by_name = {option.name: option for option in cli.params}
    assert by_name["base_sha"].envvar == "BASE_SHA"
    assert by_name["base_sha"].required is True
    assert by_name["head_sha"].envvar == "HEAD_SHA"
    assert by_name["head_sha"].required is True
