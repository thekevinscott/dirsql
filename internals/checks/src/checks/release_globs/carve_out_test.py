"""Colocated unit tests for the publish-glob carve-out (#944)."""

from unittest import mock

from checks.release_globs.carve_out import carve_out


def describe_carve_out():
    def it_pairs_a_root_file_pattern_with_a_subdirectory_pattern():
        assert carve_out("packages/rust") == [
            "packages/rust/!(testing-conventions.toml)",
            "packages/rust/!(changelog.d|migrations.d|e2e-attestations)/**",
        ]

    def it_reads_the_non_shipping_file_names_off_the_vocabulary():
        with mock.patch("checks.release_globs.carve_out.NON_SHIPPING_FILES", ("a", "b")):
            assert carve_out("p/q")[0] == "p/q/!(a|b)"

    def it_reads_the_non_shipping_directory_names_off_the_vocabulary():
        with mock.patch("checks.release_globs.carve_out.NON_SHIPPING_DIRS", ("a", "b")):
            assert carve_out("p/q")[1] == "p/q/!(a|b)/**"
