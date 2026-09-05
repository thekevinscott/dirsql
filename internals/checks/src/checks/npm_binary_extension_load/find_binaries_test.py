import os
from unittest import mock

from checks.npm_binary_extension_load.find_binaries import BIN_NAME, find_binaries


def describe_bin_name():
    def it_is_the_bundled_cli_basename():
        assert BIN_NAME == "dirsql"


def describe_find_binaries():
    def collects_matching_basenames_sorted():
        walker = mock.Mock(
            return_value=[
                ("dist/b", [], ["dirsql", "README.md"]),
                ("dist/a", [], ["dirsql"]),
            ]
        )
        assert find_binaries("dist", walker) == [
            os.path.join("dist/a", "dirsql"),
            os.path.join("dist/b", "dirsql"),
        ]
        walker.assert_called_once_with("dist")

    def matches_by_value_not_identity():
        # A name `os.walk` hands back is a fresh string object, never the
        # interned BIN_NAME literal -- an identity comparison would find
        # nothing in the real artifact tree.
        name = "".join(["dir", "sql"])
        assert name is not BIN_NAME
        walker = mock.Mock(return_value=[("dist", [], [name])])
        assert find_binaries("dist", walker) == [os.path.join("dist", "dirsql")]

    def ignores_other_names():
        walker = mock.Mock(return_value=[("dist", [], ["dirsql.exe", "notes.txt"])])
        assert find_binaries("dist", walker) == []

    def empty_walk_is_empty():
        walker = mock.Mock(return_value=[])
        assert find_binaries("dist", walker) == []

    def it_defaults_to_the_real_walk(tmp_path):
        (tmp_path / "sub").mkdir()
        (tmp_path / "sub" / "dirsql").write_text("")
        assert find_binaries(str(tmp_path)) == [str(tmp_path / "sub" / "dirsql")]
