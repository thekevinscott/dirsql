"""Colocated unit tests for the node flow's exit-code guard (a pure check)."""
from unittest import mock

import pytest

from distcheck.node_flow.require_zero import DistcheckError, require_zero


def test_require_zero_passes_on_success():
    assert require_zero(mock.Mock(returncode=0), "boom") is None


def test_require_zero_raises_on_failure():
    for rc in (1, -1):  # positive and signal-style negative exit codes
        with pytest.raises(DistcheckError, match="boom"):
            require_zero(mock.Mock(returncode=rc), "boom")
