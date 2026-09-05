from checks.changelog_gate.fragment import FRAGMENT_NAME, fragment


def describe_fragment():
    def splits_a_changelog_fragment_into_package_and_filename():
        assert fragment("packages/ts/changelog.d/2026-07-13-fix.md") == (
            "packages/ts",
            "2026-07-13-fix.md",
        )

    def splits_a_migrations_fragment():
        assert fragment("packages/rust/migrations.d/2026-07-13-break.md") == (
            "packages/rust",
            "2026-07-13-break.md",
        )

    def splits_a_plugin_fragment():
        assert fragment("plugins/dirsql-plugin-embeddings/changelog.d/2026-08-12-fix.md") == (
            "plugins/dirsql-plugin-embeddings",
            "2026-08-12-fix.md",
        )

    def none_for_a_fragment_dir_outside_a_package_root():
        assert fragment("docs/changelog.d/notes.md") is None

    def none_for_a_nested_path_inside_a_fragment_dir():
        assert fragment("packages/rust/changelog.d/sub/bad-name.md") is None

    def none_for_a_file_beside_the_fragment_dir():
        assert fragment("packages/rust/2026-07-13-fix.md") is None


def describe_fragment_name():
    def matches_an_iso_date_and_kebab_slug():
        assert FRAGMENT_NAME.fullmatch("2026-07-13-fix-cascade.md")

    def requires_a_slug_after_the_date():
        assert not FRAGMENT_NAME.fullmatch("2026-07-13.md")

    def rejects_uppercase_and_a_wrong_extension():
        assert not FRAGMENT_NAME.fullmatch("2026-07-13-Fix.md")
        assert not FRAGMENT_NAME.fullmatch("2026-07-13-fix.txt")
