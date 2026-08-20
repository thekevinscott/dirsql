from checks.changelog_gate.decide import (
    added_fragments,
    changed_packages,
    code_touched,
    has_skip_trailer,
    malformed_fragments,
)


def describe_has_skip_trailer():
    def detects_a_skip_changelog_line():
        assert has_skip_trailer("feat: thing\n\nskip-changelog: internal refactor")

    def is_case_insensitive():
        assert has_skip_trailer("Skip-Changelog: yes")

    def false_when_absent():
        assert not has_skip_trailer("feat: add a public method\n\nbody text")

    def must_start_a_line():
        assert not has_skip_trailer("see skip-changelog: in the docs")

    def false_for_empty_input():
        assert not has_skip_trailer("")


def describe_changed_packages():
    def collects_unique_and_sorted():
        assert changed_packages(
            [
                "packages/python/foo.py",
                "packages/ts/src/bar.ts",
                "packages/python/baz.py",
                "README.md",
                "packages",  # too short to name a package
            ]
        ) == ["packages/python", "packages/ts"]

    def a_two_segment_path_still_names_its_package():
        assert changed_packages(["packages/rust"]) == ["packages/rust"]

    def collects_plugins_alongside_packages():
        assert changed_packages(
            [
                "packages/rust/src/lib.rs",
                "plugins/dirsql-plugin-embeddings/src/x.py",
                "plugins",  # too short to name a package
            ]
        ) == ["packages/rust", "plugins/dirsql-plugin-embeddings"]

    def a_two_segment_plugin_path_still_names_its_package():
        assert changed_packages(["plugins/dirsql-plugin-embeddings"]) == [
            "plugins/dirsql-plugin-embeddings"
        ]

    def none_outside_the_package_roots():
        assert (
            changed_packages(["README.md", "docs/x.md", ".github/workflows/ci.yml"])
            == []
        )

    def a_top_dir_sorting_after_packages_is_still_excluded():
        # `tools` > `packages` lexically; only an exact root segment counts.
        assert changed_packages(["tools/x.md"]) == []

    def a_dir_that_merely_prefixes_a_root_is_excluded():
        assert changed_packages(["pluginsX/foo/x.py", "packagesX/foo/x.py"]) == []

    def empty_for_no_paths():
        assert changed_packages([]) == []


def describe_code_touched():
    def true_for_source():
        assert code_touched(["packages/python/dirsql/core.py"], "packages/python")

    def true_for_rust_source():
        assert code_touched(["packages/rust/src/lib.rs"], "packages/rust")

    def true_for_ts_napi_source():
        assert code_touched(["packages/ts/napi/src/lib.rs"], "packages/ts")

    def true_for_plugin_source():
        assert code_touched(
            ["plugins/dirsql-plugin-embeddings/src/dirsql_plugin_embeddings/cli.py"],
            "plugins/dirsql-plugin-embeddings",
        )

    def false_for_pointer_stubs():
        assert not code_touched(["packages/python/CHANGELOG.md"], "packages/python")
        assert not code_touched(["packages/python/MIGRATIONS.md"], "packages/python")

    def false_for_fragment_files():
        assert not code_touched(
            ["packages/rust/changelog.d/2026-07-13-fix.md"], "packages/rust"
        )
        assert not code_touched(
            ["packages/rust/migrations.d/2026-07-13-break.md"], "packages/rust"
        )

    def false_for_plugin_fragment_files():
        assert not code_touched(
            ["plugins/dirsql-plugin-embeddings/changelog.d/2026-08-12-fix.md"],
            "plugins/dirsql-plugin-embeddings",
        )
        assert not code_touched(
            ["plugins/dirsql-plugin-embeddings/migrations.d/2026-08-10-break.md"],
            "plugins/dirsql-plugin-embeddings",
        )

    def false_for_underscore_python_tests():
        assert not code_touched(
            ["packages/python/dirsql/core_test.py"], "packages/python"
        )

    def false_for_dot_test_ts_sources():
        assert not code_touched(["packages/ts/src/bar.test.ts"], "packages/ts")
        assert not code_touched(["packages/ts/src/bar.spec.tsx"], "packages/ts")

    def false_for_test_directories():
        assert not code_touched(
            ["packages/python/tests/conftest.py"], "packages/python"
        )
        assert not code_touched(["packages/rust/tests/cli.rs"], "packages/rust")

    def false_for_plugin_test_files():
        assert not code_touched(
            ["plugins/dirsql-plugin-embeddings/tests/e2e/search_cli_test.py"],
            "plugins/dirsql-plugin-embeddings",
        )
        assert not code_touched(
            [
                "plugins/dirsql-plugin-embeddings/src/dirsql_plugin_embeddings/"
                "cli_test.py"
            ],
            "plugins/dirsql-plugin-embeddings",
        )

    def false_for_e2e_attestation_receipts():
        assert not code_touched(
            ["packages/python/e2e-attestations/claude-branch-611.json"],
            "packages/python",
        )
        assert not code_touched(
            ["packages/ts/e2e-attestations/claude-branch-611.json"], "packages/ts"
        )
        assert not code_touched(
            ["plugins/dirsql-plugin-embeddings/e2e-attestations/claude-804.json"],
            "plugins/dirsql-plugin-embeddings",
        )

    def true_for_a_dir_that_merely_prefixes_e2e_attestations():
        # A trailing `/` is required: `e2e-attestationsX/` is a real source dir,
        # not the receipts folder, so it still counts as a code change.
        assert code_touched(
            ["packages/python/e2e-attestationsX/foo.py"], "packages/python"
        )

    def false_for_the_package_root_gate_config():
        assert not code_touched(
            ["packages/python/testing-conventions.toml"], "packages/python"
        )
        assert not code_touched(["packages/ts/testing-conventions.toml"], "packages/ts")
        assert not code_touched(
            ["plugins/dirsql-plugin-embeddings/testing-conventions.toml"],
            "plugins/dirsql-plugin-embeddings",
        )

    def true_for_a_gate_config_that_is_not_at_the_package_root():
        # Package-root only: a config nested deeper is not the CI-only file
        # class, and a longer filename that merely ends in it is another file.
        assert code_touched(
            ["packages/python/dirsql/testing-conventions.toml"], "packages/python"
        )
        assert code_touched(["packages/ts/my-testing-conventions.toml"], "packages/ts")

    def ignores_other_packages():
        assert not code_touched(["packages/ts/src/bar.ts"], "packages/python")

    def ignores_a_same_named_package_under_another_root():
        assert not code_touched(["plugins/ts/src/bar.ts"], "packages/ts")

    def false_when_only_exempt_files_change():
        assert not code_touched(
            ["packages/python/CHANGELOG.md", "packages/python/dirsql/x_test.py"],
            "packages/python",
        )


def describe_added_fragments():
    def a_changelog_fragment_counts():
        added = ["packages/ts/changelog.d/2026-07-13-repeatable-config.md"]
        assert added_fragments(added, "packages/ts") == added

    def a_migrations_fragment_counts():
        added = ["packages/python/migrations.d/2026-07-13-rename-config-key.md"]
        assert added_fragments(added, "packages/python") == added

    def a_plugin_changelog_fragment_counts():
        added = ["plugins/dirsql-plugin-embeddings/changelog.d/2026-08-12-fix.md"]
        assert added_fragments(added, "plugins/dirsql-plugin-embeddings") == added

    def a_plugin_migrations_fragment_counts():
        added = ["plugins/dirsql-plugin-embeddings/migrations.d/2026-08-10-break.md"]
        assert added_fragments(added, "plugins/dirsql-plugin-embeddings") == added

    def a_fragment_in_another_package_does_not_count():
        added = ["packages/ts/changelog.d/2026-07-13-fix.md"]
        assert added_fragments(added, "packages/python") == []

    def a_fragment_under_another_root_does_not_count():
        added = ["packages/ts/changelog.d/2026-07-13-fix.md"]
        assert added_fragments(added, "plugins/ts") == []

    def a_fragment_whose_package_sorts_before_the_target_does_not_count():
        # `python` < `ts` lexically; membership must be exact equality.
        added = ["packages/python/changelog.d/2026-07-13-fix.md"]
        assert added_fragments(added, "packages/ts") == []

    def requires_a_slug_after_the_date():
        assert (
            added_fragments(
                ["packages/rust/changelog.d/2026-07-13.md"], "packages/rust"
            )
            == []
        )

    def ignores_paths_outside_fragment_dirs():
        added = [
            "packages/rust/2026-07-13-fix.md",
            "packages/rust/changelog.d/nested/2026-07-13-fix.md",
        ]
        assert added_fragments(added, "packages/rust") == []


def describe_malformed_fragments():
    def flags_bad_names():
        changed = [
            "packages/rust/changelog.d/Fix.md",  # no date, uppercase
            "packages/rust/migrations.d/2026-07-13-fix.txt",  # wrong extension
        ]
        assert malformed_fragments(changed) == changed

    def flags_bad_plugin_names():
        changed = [
            "plugins/dirsql-plugin-embeddings/changelog.d/Fix.md",
            "plugins/dirsql-plugin-embeddings/migrations.d/2026-08-10-break.txt",
        ]
        assert malformed_fragments(changed) == changed

    def allows_wellformed_and_readme():
        changed = [
            "packages/rust/changelog.d/2026-07-13-fix-cascade.md",
            "packages/rust/changelog.d/README.md",
            "packages/ts/migrations.d/README.md",
            "plugins/dirsql-plugin-embeddings/changelog.d/2026-08-12-fix.md",
            "plugins/dirsql-plugin-embeddings/changelog.d/README.md",
        ]
        assert malformed_fragments(changed) == []

    def ignores_files_outside_fragment_dirs():
        assert malformed_fragments(["packages/rust/src/lib.rs", "README.md"]) == []

    def ignores_a_fragment_dir_outside_a_package_root():
        assert malformed_fragments(["docs/changelog.d/notes.md"]) == []

    def ignores_nested_files_inside_a_fragment_dir():
        assert malformed_fragments(["packages/rust/changelog.d/sub/bad-name.md"]) == []
