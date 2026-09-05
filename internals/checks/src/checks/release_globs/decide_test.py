"""Colocated unit tests for the release-globs vocabulary (#944)."""

from checks.release_globs.decide import (
    NON_SHIPPING_DIRS,
    NON_SHIPPING_FILES,
    PUBLISHED_ROOTS,
    is_wildcard,
    negations,
)


def describe_non_shipping_names():
    def it_names_the_fragment_receipt_and_gate_config_directories():
        assert NON_SHIPPING_DIRS == ("changelog.d", "migrations.d", "e2e-attestations")

    def it_names_the_per_package_gate_config_file():
        assert NON_SHIPPING_FILES == ("testing-conventions.toml",)


def describe_published_roots():
    def it_names_the_two_directories_holding_published_packages():
        assert PUBLISHED_ROOTS == ("packages", "plugins")


def describe_is_wildcard():
    def it_is_true_for_a_pattern():
        assert is_wildcard("packages/rust/**") is True

    def it_is_true_for_an_extglob():
        assert is_wildcard("packages/rust/!(testing-conventions.toml)") is True

    def it_is_false_for_a_literal_path():
        assert is_wildcard("Cargo.toml") is False


def describe_negations():
    def it_collects_leading_bang_entries():
        assert negations(["packages/rust/**", "!packages/rust/changelog.d/**"]) == [
            "!packages/rust/changelog.d/**"
        ]

    def it_is_empty_when_every_entry_is_positive():
        assert negations(["packages/rust/**"]) == []
