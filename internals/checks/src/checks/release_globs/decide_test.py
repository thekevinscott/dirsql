"""Colocated unit tests for the release-globs decision logic (#944)."""

from checks.release_globs.decide import (
    NON_SHIPPING_DIRS,
    NON_SHIPPING_FILES,
    carve_out,
    glob_problems,
    is_wildcard,
    negations,
    subtree_globs,
    subtree_root,
    subtree_roots,
    unprechecked,
)

RUST = carve_out("packages/rust")


def describe_carve_out():
    def it_pairs_a_root_file_pattern_with_a_subdirectory_pattern():
        assert carve_out("packages/rust") == [
            "packages/rust/!(testing-conventions.toml)",
            "packages/rust/!(changelog.d|migrations.d|e2e-attestations)/**",
        ]

    def it_names_every_non_shipping_entry():
        first, second = carve_out("p/q")
        assert all(name in second for name in NON_SHIPPING_DIRS)
        assert all(name in first for name in NON_SHIPPING_FILES)


def describe_subtree_root():
    def it_reads_the_package_root_off_a_recursive_glob():
        assert subtree_root("packages/rust/**") == "packages/rust"

    def it_reads_the_same_root_off_an_extglob_carve_out():
        assert subtree_root(RUST[1]) == "packages/rust"

    def it_accepts_the_plugins_root():
        assert subtree_root("plugins/dirsql-plugin-embeddings/**") == (
            "plugins/dirsql-plugin-embeddings"
        )

    def it_ignores_a_repo_root_file():
        assert subtree_root("Cargo.toml") is None

    def it_ignores_a_two_segment_path_that_names_no_content():
        assert subtree_root("packages/rust") is None

    def it_ignores_a_directory_that_holds_no_published_package():
        assert subtree_root("internals/checks/src/**") is None


def describe_is_wildcard():
    def it_is_true_for_a_pattern():
        assert is_wildcard("packages/rust/**") is True

    def it_is_true_for_an_extglob():
        assert is_wildcard(RUST[0]) is True

    def it_is_false_for_a_literal_path():
        assert is_wildcard("Cargo.toml") is False


def describe_negations():
    def it_collects_leading_bang_entries():
        assert negations(["packages/rust/**", "!packages/rust/changelog.d/**"]) == [
            "!packages/rust/changelog.d/**"
        ]

    def it_is_empty_when_every_entry_is_positive():
        assert negations(["packages/rust/**"]) == []


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


def describe_subtree_roots():
    def it_dedupes_and_sorts_the_roots_the_globs_reach_into():
        assert subtree_roots(["packages/ts/**", *RUST, "Cargo.lock"]) == [
            "packages/rust",
            "packages/ts",
        ]

    def it_is_empty_for_repo_root_globs_only():
        assert subtree_roots(["Cargo.toml", "Cargo.lock"]) == []


def describe_glob_problems():
    def it_accepts_a_package_whose_globs_carve_out_the_non_shipping_paths():
        assert glob_problems([{"name": "dirsql-rust", "globs": [*RUST, "Cargo.toml"]}]) == []

    def it_rejects_a_bare_recursive_subtree_glob():
        problems = glob_problems([{"name": "dirsql-rust", "globs": ["packages/rust/**"]}])
        assert len(problems) == 1
        assert "publish globs for packages/rust are packages/rust/**" in problems[0]
        assert RUST[1] in problems[0]

    def it_rejects_a_carve_out_missing_the_root_file_pattern():
        problems = glob_problems([{"name": "dirsql-rust", "globs": [RUST[1]]}])
        assert len(problems) == 1
        assert RUST[0] in problems[0]

    def it_rejects_a_leading_bang_negation_and_explains_the_extglob_form():
        problems = glob_problems(
            [{"name": "dirsql-rust", "globs": [*RUST, "!packages/rust/changelog.d/**"]}]
        )
        assert len(problems) == 1
        assert "leading-`!` negation" in problems[0]
        assert "matches every path outside its own subtree" in problems[0]
        # The suggestion has to be the subdirectory pattern -- the fragment dirs
        # the author was reaching for live there, not in the root-file pattern.
        assert '"<root>/!(changelog.d|migrations.d|e2e-attestations)/**"' in problems[0]

    def it_reports_every_root_a_package_globs():
        problems = glob_problems(
            [{"name": "dirsql-py", "globs": ["packages/python/**", "packages/rust/**"]}]
        )
        assert len(problems) == 2

    def it_reports_every_package():
        problems = glob_problems(
            [{"name": "a", "globs": ["packages/rust/**"]}, {"name": "b", "globs": ["packages/ts/**"]}]
        )
        assert len(problems) == 2

    def it_names_an_unnamed_package_rather_than_crashing():
        assert glob_problems([{"globs": ["packages/rust/**"]}])[0].startswith("<unnamed>:")

    def it_accepts_a_package_declaring_no_globs():
        assert glob_problems([{"name": "a"}]) == []

    def it_accepts_an_empty_package_list():
        assert glob_problems([]) == []


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
