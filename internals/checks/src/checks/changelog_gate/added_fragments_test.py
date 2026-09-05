from checks.changelog_gate.added_fragments import added_fragments


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
