"""Colocated unit tests for collecting the subtrees a glob list reaches (#944)."""

from checks.release_globs.subtree_roots import subtree_roots

RUST = [
    "packages/rust/!(testing-conventions.toml)",
    "packages/rust/!(changelog.d|migrations.d|e2e-attestations)/**",
]


def describe_subtree_roots():
    def it_dedupes_and_sorts_the_roots_the_globs_reach_into():
        assert subtree_roots(["packages/ts/**", *RUST, "Cargo.lock"]) == [
            "packages/rust",
            "packages/ts",
        ]

    def it_omits_a_negation_reaching_into_a_root():
        assert subtree_roots(["!packages/rust/changelog.d/**"]) == []

    def it_omits_a_literal_path_inside_a_root():
        assert subtree_roots(["packages/rust/Cargo.toml"]) == []

    def it_is_empty_for_repo_root_globs_only():
        assert subtree_roots(["Cargo.toml", "Cargo.lock"]) == []
