from checks.changelog_gate.decide import (
    any_sdk_code_changed,
    count_added_lines,
    extract_skip_trailers,
    is_sdk_code_change,
)


def describe_is_sdk_code_change():
    def rust_core_source_counts():
        assert is_sdk_code_change("packages/rust/src/lib.rs") is True

    def python_binding_source_counts():
        assert is_sdk_code_change("packages/python/src/lib.rs") is True

    def python_package_source_counts():
        assert is_sdk_code_change("packages/python/dirsql/table.py") is True

    def python_package_test_file_is_excluded():
        assert is_sdk_code_change("packages/python/dirsql/table_test.py") is False

    def ts_napi_binding_source_counts():
        assert is_sdk_code_change("packages/ts/napi/src/lib.rs") is True

    def ts_package_source_counts():
        assert is_sdk_code_change("packages/ts/src/table.ts") is True

    def ts_package_test_file_is_excluded():
        assert is_sdk_code_change("packages/ts/src/table.test.ts") is False

    def ts_package_spec_file_is_excluded():
        assert is_sdk_code_change("packages/ts/src/table.spec.ts") is False

    def top_level_cargo_toml_counts():
        assert is_sdk_code_change("Cargo.toml") is True

    def top_level_cargo_lock_counts():
        assert is_sdk_code_change("Cargo.lock") is True

    def rust_crate_manifest_counts():
        assert is_sdk_code_change("packages/rust/Cargo.toml") is True

    def ts_napi_crate_manifest_counts():
        assert is_sdk_code_change("packages/ts/napi/Cargo.toml") is True

    def python_crate_manifest_counts():
        assert is_sdk_code_change("packages/python/Cargo.toml") is True

    def docs_do_not_count():
        assert is_sdk_code_change("docs/guide.md") is False

    def workflow_files_do_not_count():
        assert is_sdk_code_change(".github/workflows/ci.yml") is False


def describe_any_sdk_code_changed():
    def true_when_any_path_counts():
        assert any_sdk_code_changed(["README.md", "packages/rust/src/lib.rs"]) is True

    def false_when_no_path_counts():
        assert any_sdk_code_changed(["README.md", "docs/guide.md"]) is False

    def false_for_an_empty_list():
        assert any_sdk_code_changed([]) is False


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


def describe_count_added_lines():
    def counts_added_non_blank_lines():
        diff = "--- a/CHANGELOG.md\n+++ b/CHANGELOG.md\n+- new entry\n context\n-old\n"
        assert count_added_lines(diff) == 1

    def excludes_the_diff_header_line():
        diff = "+++ b/CHANGELOG.md\n"
        assert count_added_lines(diff) == 0

    def excludes_whitespace_only_additions():
        diff = "+   \n"
        assert count_added_lines(diff) == 0

    def excludes_a_bare_plus_line():
        diff = "+\n"
        assert count_added_lines(diff) == 0

    def returns_zero_for_an_empty_diff():
        assert count_added_lines("") == 0
