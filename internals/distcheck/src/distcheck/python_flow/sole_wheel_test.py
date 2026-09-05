"""Colocated unit tests for wheel selection (a pure function, nothing to mock)."""
import pytest

from distcheck.python_flow.sole_wheel import DistcheckError, sole_wheel

_WHEEL = "dirsql-1.0-cp311-abi3-linux_x86_64.whl"


def test_sole_wheel_returns_the_single_wheel():
    assert sole_wheel(["notes.txt", _WHEEL]) == _WHEEL


def test_sole_wheel_rejects_none():
    with pytest.raises(DistcheckError, match="exactly one wheel"):
        sole_wheel(["notes.txt"])


def test_sole_wheel_rejects_many():
    with pytest.raises(DistcheckError, match="exactly one wheel"):
        sole_wheel(["a.whl", "b.whl"])


def test_sole_wheel_reports_the_candidates_it_saw():
    with pytest.raises(DistcheckError) as raised:
        sole_wheel(["a.whl", "b.whl"])
    assert "['a.whl', 'b.whl']" in str(raised.value)
