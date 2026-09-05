from checks.changelog_gate.code_touched import code_touched


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
