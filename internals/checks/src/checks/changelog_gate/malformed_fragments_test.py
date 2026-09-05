from checks.changelog_gate.malformed_fragments import malformed_fragments


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
