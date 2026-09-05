"""Colocated unit tests for the arch mapping (a pure lookup, nothing to mock)."""
import pytest

from distcheck.node_flow.arch import _ARCH, to_node_arch


def test_to_node_arch_maps_known_and_is_case_insensitive():
    assert to_node_arch("x86_64") == "x64"
    assert to_node_arch("AMD64") == "x64"
    assert to_node_arch("arm64") == "arm64"
    assert to_node_arch("aarch64") == "arm64"


def test_to_node_arch_rejects_unknown():
    with pytest.raises(ValueError, match="unsupported machine"):
        to_node_arch("mips")


def test_arch_table_spells_both_vendor_names_for_each_node_arch():
    # The table exists because uname and node disagree; a row lost here is a
    # host that stops resolving, so pin the pairs rather than the size.
    assert _ARCH == {
        "x86_64": "x64",
        "amd64": "x64",
        "arm64": "arm64",
        "aarch64": "arm64",
    }
