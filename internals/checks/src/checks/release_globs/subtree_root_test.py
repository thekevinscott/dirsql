"""Colocated unit tests for reading a glob's package subtree (#944)."""

from checks.release_globs.subtree_root import subtree_root


def describe_subtree_root():
    def it_reads_the_package_root_off_a_recursive_glob():
        assert subtree_root("packages/rust/**") == "packages/rust"

    def it_reads_the_same_root_off_an_extglob_carve_out():
        assert subtree_root("packages/rust/!(changelog.d)/**") == "packages/rust"

    def it_accepts_the_plugins_root():
        assert subtree_root("plugins/dirsql-plugin-embeddings/**") == (
            "plugins/dirsql-plugin-embeddings"
        )

    def it_ignores_a_repo_root_file():
        assert subtree_root("Cargo.toml") is None

    def it_ignores_a_two_segment_path_that_names_no_content():
        assert subtree_root("packages/rust") is None

    def it_ignores_a_directory_that_holds_no_published_package():
        assert subtree_root("internals/checks/src/**") is None
