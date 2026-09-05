"""Colocated unit tests for the staged-artifact listing (#790)."""

from checks.artifact_completeness.subdirectories import subdirectories


def describe_subdirectories():
    def it_lists_only_directories_sorted(tmp_path):
        (tmp_path / "b").mkdir()
        (tmp_path / "a").mkdir()
        (tmp_path / "note.txt").write_text("x")
        assert subdirectories(str(tmp_path)) == ["a", "b"]

    def it_returns_empty_when_the_dist_dir_does_not_exist(tmp_path):
        assert subdirectories(str(tmp_path / "nope")) == []
