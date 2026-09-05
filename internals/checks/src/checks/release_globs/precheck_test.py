"""Colocated unit tests for the precheck half of the release-globs invariant (#944)."""

from checks.release_globs.precheck import subtree_root, unprechecked


def describe_collaborators():
    def it_resolves_subtree_root_from_its_own_module():
        assert subtree_root.__module__ == "checks.release_globs.subtree_root"


def describe_unprechecked():
    def it_accepts_an_exclusion_naming_a_non_shipping_directory():
        assert unprechecked(["!packages/rust/changelog.d/**"]) == []

    def it_accepts_an_exclusion_naming_a_non_shipping_root_file():
        assert unprechecked(["!packages/python/testing-conventions.toml"]) == []

    def it_ignores_an_exclusion_outside_every_published_package():
        assert unprechecked(["!docs/**"]) == []

    def it_rejects_an_exclusion_for_a_directory_that_still_publishes():
        problems = unprechecked(["!plugins/dirsql-plugin-embeddings/tests/**"])
        assert len(problems) == 1
        assert "tests is not one of" in problems[0]
        assert "plugins/dirsql-plugin-embeddings" in problems[0]

    def it_rejects_an_exclusion_for_a_root_file_that_still_publishes():
        assert len(unprechecked(["!packages/ts/biome.json"])) == 1

    def it_rejects_a_non_shipping_name_used_at_the_wrong_depth():
        # `changelog.d` without the `/**` tail is a file exclusion, and no file
        # by that name is carved out of the publish globs.
        assert len(unprechecked(["!packages/rust/changelog.d"])) == 1

    def it_reports_every_offending_exclusion():
        assert len(unprechecked(["!packages/ts/tools/**", "!packages/ts/biome.json"])) == 2

    def it_keeps_scanning_past_an_exclusion_outside_every_package():
        assert len(unprechecked(["!docs/**", "!packages/ts/biome.json"])) == 1

    def it_keeps_scanning_past_an_accepted_directory_exclusion():
        assert (
            len(unprechecked(["!packages/rust/changelog.d/**", "!packages/ts/biome.json"])) == 1
        )

    def it_keeps_scanning_past_an_accepted_root_file_exclusion():
        exclusions = ["!packages/python/testing-conventions.toml", "!packages/ts/biome.json"]
        assert len(unprechecked(exclusions)) == 1

    def it_accepts_an_empty_exclusion_list():
        assert unprechecked([]) == []
