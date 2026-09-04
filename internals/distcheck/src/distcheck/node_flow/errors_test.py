"""Colocated unit tests for the node distcheck error type (no collaborators)."""
import pytest

from distcheck.node_flow.errors import DistcheckError


def test_distcheck_error_is_a_runtime_error():
    assert issubclass(DistcheckError, RuntimeError)


def test_distcheck_error_carries_its_diagnostic():
    with pytest.raises(DistcheckError, match="boom") as raised:
        raise DistcheckError("boom")
    assert str(raised.value) == "boom"
