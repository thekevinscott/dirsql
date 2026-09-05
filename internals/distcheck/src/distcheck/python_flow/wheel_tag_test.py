"""Colocated unit tests for the abi3 wheel-tag check (pure string inspection)."""
import pytest

from distcheck.python_flow.wheel_tag import DistcheckError, check_wheel_tag

_WHEEL = "dirsql-1.0-cp311-abi3-linux_x86_64.whl"


def test_check_wheel_tag_accepts_abi3_cpython():
    assert check_wheel_tag(_WHEEL) is None


def test_check_wheel_tag_rejects_non_abi3():
    with pytest.raises(DistcheckError, match="abi3 wheel tag"):
        check_wheel_tag("dirsql-1.0-cp311-cp311-linux_x86_64.whl")


def test_check_wheel_tag_rejects_non_cpython_interpreter():
    with pytest.raises(DistcheckError, match="interpreter tag"):
        check_wheel_tag("dirsql-1.0-xy-abi3-linux_x86_64.whl")


def test_check_wheel_tag_reads_the_interpreter_from_the_third_field():
    # A `cp3`-looking string anywhere else must not satisfy the check.
    with pytest.raises(DistcheckError, match="interpreter tag"):
        check_wheel_tag("cp311-1.0-py2-abi3-linux_x86_64.whl")
