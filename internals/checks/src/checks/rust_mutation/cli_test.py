"""Colocated unit tests for the rust-mutation command (isolation -- no subprocess).

Driven through `.callback` (the undecorated function); `run` is mocked at its import site so
no git or cargo-mutants collaborator runs during the test.
"""
from unittest import mock

import pytest

from checks.rust_mutation.cli import cli


def test_exits_with_the_gate_return_code():
    with mock.patch("checks.rust_mutation.cli.run", return_value=0) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(base="origin/main")
        run.assert_called_once_with("origin/main")
        assert exc_info.value.code == 0


def test_propagates_a_nonzero_return_code():
    with mock.patch("checks.rust_mutation.cli.run", return_value=2) as run:
        with pytest.raises(SystemExit) as exc_info:
            cli.callback(base="deadbeef")
        run.assert_called_once_with("deadbeef")
        assert exc_info.value.code == 2


def test_base_defaults_to_origin_main():
    (option,) = cli.params
    assert option.name == "base"
    assert option.default == "origin/main"
    assert option.show_default is True
