"""Colocated unit tests for selecting one subtree's wildcard entries (#944)."""

from checks.release_globs.subtree_globs import subtree_globs

RUST = [
    "packages/rust/!(testing-conventions.toml)",
    "packages/rust/!(changelog.d|migrations.d|e2e-attestations)/**",
]


def describe_subtree_globs():
    def it_returns_the_entries_reaching_into_one_root_sorted():
        assert subtree_globs([*reversed(RUST), "Cargo.toml"], "packages/rust") == sorted(RUST)

    def it_omits_entries_reaching_into_a_different_root():
        # Both a root that sorts before `packages/rust` and one that sorts
        # after: the root comparison is an identity, not an ordering.
        assert subtree_globs(["packages/python/**", "packages/ts/**"], "packages/rust") == []

    def it_omits_a_literal_path_inside_the_root():
        assert subtree_globs(["packages/rust/Cargo.toml"], "packages/rust") == []

    def it_omits_a_negation_so_it_is_reported_once_as_a_negation():
        assert subtree_globs(["!packages/rust/changelog.d/**"], "packages/rust") == []
