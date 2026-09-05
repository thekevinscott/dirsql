"""Colocated unit tests for the declared-deps manifest split (#782).

Isolation: `requirement_name` runs for real -- it is a pure string-in /
string-out normalization.
"""

from checks.declared_deps.declared import declared

MANIFEST = {
    "project": {"dependencies": ["click>=8.1", "PyYAML==6.0"]},
    "dependency-groups": {"dev": ["pytest>=8"]},
}


def describe_declared():
    def it_splits_runtime_from_dev_and_normalizes_both():
        assert declared(MANIFEST) == ({"click", "pyyaml"}, {"pytest"})

    def it_returns_empty_sets_for_a_manifest_declaring_nothing():
        assert declared({}) == (set(), set())
