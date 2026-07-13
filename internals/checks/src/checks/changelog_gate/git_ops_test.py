from unittest import mock

from checks.changelog_gate.git_ops import (
    added_files,
    changed_files,
    commit_messages,
)


def describe_changed_files():
    def parses_stdout_into_lines_via_three_dot_diff():
        runner = mock.Mock(return_value=mock.Mock(stdout="a.py\nb.py\n"))
        assert changed_files("base", "head", runner=runner) == ["a.py", "b.py"]
        runner.assert_called_once_with(
            ["git", "diff", "--name-only", "base...head"],
            capture_output=True,
            text=True,
            check=True,
        )

    def drops_blank_lines():
        runner = mock.Mock(return_value=mock.Mock(stdout="a.py\n\n"))
        assert changed_files("base", "head", runner=runner) == ["a.py"]


def describe_added_files():
    def parses_added_only_diff():
        runner = mock.Mock(return_value=mock.Mock(stdout="new.md\n"))
        assert added_files("base", "head", runner=runner) == ["new.md"]
        runner.assert_called_once_with(
            ["git", "diff", "--name-only", "--diff-filter=A", "base...head"],
            capture_output=True,
            text=True,
            check=True,
        )


def describe_commit_messages():
    def returns_raw_bodies_via_two_dot_range():
        runner = mock.Mock(return_value=mock.Mock(stdout="feat: x\n\nskip-changelog: y\n"))
        assert (
            commit_messages("base", "head", runner=runner)
            == "feat: x\n\nskip-changelog: y\n"
        )
        runner.assert_called_once_with(
            ["git", "log", "--format=%B", "base..head"],
            capture_output=True,
            text=True,
            check=True,
        )
