from unittest import mock

import pytest

from checks.probe.require_zero import ProbeError, require_zero


def _result(returncode):
    return mock.Mock(returncode=returncode)


def describe_require_zero():
    def zero_passes():
        assert require_zero(_result(0), "boom") is None

    def positive_code_raises():
        with pytest.raises(ProbeError, match="boom"):
            require_zero(_result(1), "boom")

    def negative_signal_code_raises():
        with pytest.raises(ProbeError, match="boom"):
            require_zero(_result(-9), "boom")
