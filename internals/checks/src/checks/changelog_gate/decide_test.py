from checks.changelog_gate.decide import (
    changed_packages,
    contains_skip_changelog_line,
    extract_skip_trailers,
    fragment_package,
    fragment_paths,
    is_valid_fragment_name,
    package_for_path,
)


def describe_package_for_path():
    def rust_core_source_is_rust():
        assert package_for_path("packages/rust/src/lib.rs") == "rust"

    def rust_crate_manifest_is_rust():
        assert package_for_path("packages/rust/Cargo.toml") == "rust"

    def top_level_cargo_toml_is_rust():
        assert package_for_path("Cargo.toml") == "rust"

    def top_level_cargo_lock_is_rust():
        assert package_for_path("Cargo.lock") == "rust"

    def python_binding_source_is_python():
        assert package_for_path("packages/python/src/lib.rs") == "python"

    def python_package_source_is_python():
        assert package_for_path("packages/python/dirsql/table.py") == "python"

    def python_crate_manifest_is_python():
        assert package_for_path("packages/python/Cargo.toml") == "python"

    def python_test_file_is_not_sdk_source():
        assert package_for_path("packages/python/dirsql/table_test.py") is None

    def ts_package_source_is_ts():
        assert package_for_path("packages/ts/src/table.ts") == "ts"

    def ts_napi_source_is_ts():
        assert package_for_path("packages/ts/napi/src/lib.rs") == "ts"

    def ts_napi_manifest_is_ts():
        assert package_for_path("packages/ts/napi/Cargo.toml") == "ts"

    def ts_test_file_is_not_sdk_source():
        assert package_for_path("packages/ts/src/table.test.ts") is None

    def ts_spec_file_is_not_sdk_source():
        assert package_for_path("packages/ts/src/table.spec.ts") is None

    def docs_are_not_sdk_source():
        assert package_for_path("docs/guide.md") is None

    def workflow_files_are_not_sdk_source():
        assert package_for_path(".github/workflows/ci.yml") is None


def describe_changed_packages():
    def collects_the_distinct_packages():
        assert changed_packages(
            [
                "packages/rust/src/lib.rs",
                "packages/python/dirsql/table.py",
                "packages/python/src/lib.rs",
                "README.md",
            ]
        ) == {"rust", "python"}

    def is_empty_when_nothing_is_sdk_source():
        assert changed_packages(["README.md", "docs/guide.md"]) == set()

    def is_empty_for_no_paths():
        assert changed_packages([]) == set()


def describe_fragment_package():
    def a_dated_fragment_maps_to_its_package():
        assert (
            fragment_package("packages/python/changelog.d/2026-07-13-repeatable.md")
            == "python"
        )

    def a_badly_named_md_fragment_still_maps_to_its_package():
        # Name validity is a separate check; location alone identifies the pkg.
        assert fragment_package("packages/ts/changelog.d/notes.md") == "ts"

    def the_dir_readme_is_not_a_fragment():
        assert fragment_package("packages/rust/changelog.d/README.md") is None

    def a_non_md_file_is_not_a_fragment():
        assert fragment_package("packages/rust/changelog.d/notes.txt") is None

    def a_file_outside_a_package_changelog_dir_is_not_a_fragment():
        assert fragment_package("changelog.d/2026-07-13-x.md") is None

    def an_unknown_package_is_not_a_fragment():
        assert fragment_package("packages/docs/changelog.d/2026-07-13-x.md") is None

    def a_nested_path_is_not_a_fragment():
        assert fragment_package("packages/rust/changelog.d/sub/2026-07-13-x.md") is None


def describe_is_valid_fragment_name():
    def a_dated_kebab_slug_is_valid():
        assert (
            is_valid_fragment_name("packages/rust/changelog.d/2026-07-13-fix-race.md")
            is True
        )

    def a_single_word_slug_is_valid():
        assert is_valid_fragment_name("packages/rust/changelog.d/2026-07-13-x.md") is True

    def a_missing_date_is_invalid():
        assert is_valid_fragment_name("packages/rust/changelog.d/fix-race.md") is False

    def an_uppercase_slug_is_invalid():
        assert (
            is_valid_fragment_name("packages/rust/changelog.d/2026-07-13-Fix.md") is False
        )

    def a_trailing_hyphen_is_invalid():
        assert (
            is_valid_fragment_name("packages/rust/changelog.d/2026-07-13-fix-.md") is False
        )


def describe_fragment_paths():
    def returns_only_fragment_paths():
        assert fragment_paths(
            [
                "packages/rust/src/lib.rs",
                "packages/rust/changelog.d/2026-07-13-x.md",
                "packages/rust/changelog.d/README.md",
            ]
        ) == ["packages/rust/changelog.d/2026-07-13-x.md"]

    def returns_empty_for_no_fragments():
        assert fragment_paths(["README.md", "CHANGELOG.md"]) == []


def describe_extract_skip_trailers():
    def filters_blank_lines():
        assert extract_skip_trailers("reason one\n\nreason two\n") == [
            "reason one",
            "reason two",
        ]

    def returns_empty_list_when_no_trailers():
        assert extract_skip_trailers("") == []

    def returns_empty_list_for_whitespace_only_output():
        assert extract_skip_trailers("\n\n") == []


def describe_contains_skip_changelog_line():
    def detects_a_skip_changelog_line():
        assert (
            contains_skip_changelog_line("feat: x\n\nskip-changelog: internal\n") is True
        )

    def detects_it_case_insensitively_and_indented():
        assert contains_skip_changelog_line("   Skip-Changelog: why\n") is True

    def false_when_no_skip_changelog_present():
        assert (
            contains_skip_changelog_line("feat: x\n\nCo-Authored-By: a <a@b.c>\n") is False
        )

    def false_for_empty_input():
        assert contains_skip_changelog_line("") is False

    def a_mention_mid_line_does_not_count():
        assert contains_skip_changelog_line("see the skip-changelog: docs\n") is False
