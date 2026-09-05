from checks.changelog_gate.is_exempt import is_exempt


def describe_is_exempt():
    def false_for_source():
        assert not is_exempt("packages/python/dirsql/core.py", "packages/python")
        assert not is_exempt("packages/rust/src/lib.rs", "packages/rust")

    def true_for_pointer_stubs():
        assert is_exempt("packages/python/CHANGELOG.md", "packages/python")
        assert is_exempt("packages/python/MIGRATIONS.md", "packages/python")

    def true_for_fragment_dirs():
        assert is_exempt("packages/rust/changelog.d/2026-07-13-fix.md", "packages/rust")
        assert is_exempt("packages/rust/migrations.d/2026-07-13-break.md", "packages/rust")

    def true_for_e2e_attestation_receipts():
        assert is_exempt("packages/ts/e2e-attestations/claude-branch.json", "packages/ts")

    def false_for_a_dir_that_merely_prefixes_e2e_attestations():
        assert not is_exempt("packages/python/e2e-attestationsX/foo.py", "packages/python")

    def true_for_the_package_root_gate_config():
        assert is_exempt("packages/ts/testing-conventions.toml", "packages/ts")

    def false_for_a_gate_config_below_the_package_root():
        assert not is_exempt("packages/ts/my-testing-conventions.toml", "packages/ts")
        assert not is_exempt(
            "packages/python/dirsql/testing-conventions.toml", "packages/python"
        )

    def true_for_colocated_unit_tests():
        assert is_exempt("packages/python/dirsql/core_test.py", "packages/python")
        assert is_exempt("packages/ts/src/bar.test.ts", "packages/ts")
        assert is_exempt("packages/ts/src/bar.spec.tsx", "packages/ts")

    def true_for_test_directories():
        assert is_exempt("packages/python/tests/conftest.py", "packages/python")
        assert is_exempt("packages/rust/tests/cli.rs", "packages/rust")

    def false_for_another_package():
        assert not is_exempt("packages/ts/CHANGELOG.md", "packages/python")

    def escapes_regex_metacharacters_in_the_package_name():
        # `plugins/dirsql-plugin-embeddings` has no metacharacter, but a `.` in
        # an unescaped package name would match any character.
        assert not is_exempt("packages/pythonX/CHANGELOG.md", "packages/python")
