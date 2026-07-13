from unittest import mock

from checks.changelog_gate.git_ops import (
    changed_files,
    commit_messages,
    skip_trailers,
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


def describe_skip_trailers():
    def returns_raw_stdout():
        runner = mock.Mock(return_value=mock.Mock(stdout="reason\n"))
        assert skip_trailers("base", "head", runner=runner) == "reason\n"
        runner.assert_called_once_with(
            [
                "git",
                "log",
                "--format=%(trailers:key=skip-changelog,valueonly)",
                "base..head",
            ],
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
