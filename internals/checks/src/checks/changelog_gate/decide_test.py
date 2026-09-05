from checks.changelog_gate.decide import changed_packages, has_skip_trailer


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
