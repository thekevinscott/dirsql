from unittest import mock

from checks.attestation_guard.git_ops import commit_messages, deleted_files


def describe_deleted_files():
    def asks_git_for_deletions_only_over_the_merge_base_range():
        runner = mock.Mock(return_value=mock.Mock(stdout="a.json\n"))
        assert deleted_files("base", "head", runner=runner) == ["a.json"]
        runner.assert_called_once_with(
            ["git", "diff", "--name-only", "--diff-filter=D", "--no-renames",
             "base...head"],
            capture_output=True,
            text=True,
            check=True,
        )

    def parses_stdout_into_lines():
        runner = mock.Mock(return_value=mock.Mock(stdout="a.json\nb.json\n"))
        assert deleted_files("base", "head", runner=runner) == ["a.json", "b.json"]

    def drops_blank_lines():
        runner = mock.Mock(return_value=mock.Mock(stdout="a.json\n\n"))
        assert deleted_files("base", "head", runner=runner) == ["a.json"]


def describe_commit_messages():
    def returns_raw_bodies_via_the_two_dot_range():
        runner = mock.Mock(return_value=mock.Mock(stdout="fix: x\n"))
        assert commit_messages("base", "head", runner=runner) == "fix: x\n"
        runner.assert_called_once_with(
            ["git", "log", "--format=%B", "base..head"],
            capture_output=True,
            text=True,
            check=True,
        )
