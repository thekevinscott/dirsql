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
        ) == ["python", "ts"]

    def a_two_segment_path_still_names_its_package():
        assert changed_packages(["packages/rust"]) == ["rust"]

    def none_outside_packages():
        assert (
            changed_packages(["README.md", "docs/x.md", ".github/workflows/ci.yml"])
            == []
        )

    def a_top_dir_sorting_after_packages_is_still_excluded():
        # `tools` > `packages` lexically; only an exact `packages` prefix counts.
        assert changed_packages(["tools/x.md"]) == []

    def empty_for_no_paths():
        assert changed_packages([]) == []


def describe_code_touched():
    def true_for_source():
        assert code_touched(["packages/python/dirsql/core.py"], "python")

    def true_for_rust_source():
        assert code_touched(["packages/rust/src/lib.rs"], "rust")

    def true_for_ts_napi_source():
        assert code_touched(["packages/ts/napi/src/lib.rs"], "ts")

    def false_for_pointer_stubs():
        assert not code_touched(["packages/python/CHANGELOG.md"], "python")
        assert not code_touched(["packages/python/MIGRATIONS.md"], "python")

    def false_for_fragment_files():
        assert not code_touched(
            ["packages/rust/changelog.d/2026-07-13-fix.md"], "rust"
        )
        assert not code_touched(
            ["packages/rust/migrations.d/2026-07-13-break.md"], "rust"
        )

    def false_for_underscore_python_tests():
        assert not code_touched(["packages/python/dirsql/core_test.py"], "python")

    def false_for_dot_test_ts_sources():
        assert not code_touched(["packages/ts/src/bar.test.ts"], "ts")
        assert not code_touched(["packages/ts/src/bar.spec.tsx"], "ts")

    def false_for_test_directories():
        assert not code_touched(["packages/python/tests/conftest.py"], "python")
        assert not code_touched(["packages/rust/tests/cli.rs"], "rust")

    def false_for_e2e_attestation_receipts():
        assert not code_touched(
            ["packages/python/e2e-attestations/claude-branch-611.json"], "python"
        )
        assert not code_touched(
            ["packages/ts/e2e-attestations/claude-branch-611.json"], "ts"
        )

    def true_for_a_dir_that_merely_prefixes_e2e_attestations():
        # A trailing `/` is required: `e2e-attestationsX/` is a real source dir,
        # not the receipts folder, so it still counts as a code change.
        assert code_touched(
            ["packages/python/e2e-attestationsX/foo.py"], "python"
        )

    def ignores_other_packages():
        assert not code_touched(["packages/ts/src/bar.ts"], "python")

    def false_when_only_exempt_files_change():
        assert not code_touched(
            ["packages/python/CHANGELOG.md", "packages/python/dirsql/x_test.py"],
            "python",
        )


def describe_added_fragments():
    def a_changelog_fragment_counts():
        added = ["packages/ts/changelog.d/2026-07-13-repeatable-config.md"]
        assert added_fragments(added, "ts") == added

    def a_migrations_fragment_counts():
        added = ["packages/python/migrations.d/2026-07-13-rename-config-key.md"]
        assert added_fragments(added, "python") == added

    def a_fragment_in_another_package_does_not_count():
        added = ["packages/ts/changelog.d/2026-07-13-fix.md"]
        assert added_fragments(added, "python") == []

    def a_fragment_whose_package_sorts_before_the_target_does_not_count():
        # `python` < `ts` lexically; membership must be exact equality.
        added = ["packages/python/changelog.d/2026-07-13-fix.md"]
        assert added_fragments(added, "ts") == []

    def requires_a_slug_after_the_date():
        assert added_fragments(["packages/rust/changelog.d/2026-07-13.md"], "rust") == []

    def ignores_paths_outside_fragment_dirs():
        added = [
            "packages/rust/2026-07-13-fix.md",
            "packages/rust/changelog.d/nested/2026-07-13-fix.md",
        ]
        assert added_fragments(added, "rust") == []


def describe_malformed_fragments():
    def flags_bad_names():
        changed = [
            "packages/rust/changelog.d/Fix.md",  # no date, uppercase
            "packages/rust/migrations.d/2026-07-13-fix.txt",  # wrong extension
        ]
        assert malformed_fragments(changed) == changed

    def allows_wellformed_and_readme():
        changed = [
            "packages/rust/changelog.d/2026-07-13-fix-cascade.md",
            "packages/rust/changelog.d/README.md",
            "packages/ts/migrations.d/README.md",
        ]
        assert malformed_fragments(changed) == []

    def ignores_files_outside_fragment_dirs():
        assert malformed_fragments(["packages/rust/src/lib.rs", "README.md"]) == []

    def ignores_nested_files_inside_a_fragment_dir():
        assert malformed_fragments(["packages/rust/changelog.d/sub/bad-name.md"]) == []
