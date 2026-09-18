from unittest import mock

from checks.git.diff_names import diff_names


def describe_diff_names():
    def parses_stdout_into_lines_via_three_dot_diff():
        runner = mock.Mock(return_value=mock.Mock(stdout="a.py\nb.py\n"))
        assert diff_names("base", "head", runner=runner) == ["a.py", "b.py"]
        runner.assert_called_once_with(
            ["git", "diff", "--name-only", "base...head"],
            capture_output=True,
            text=True,
            check=True,
        )

    def passes_flags_through_ahead_of_the_range():
        runner = mock.Mock(return_value=mock.Mock(stdout="new.md\n"))
        assert diff_names(
            "base", "head", ("--diff-filter=A", "--no-renames"), runner
        ) == ["new.md"]
        runner.assert_called_once_with(
            [
                "git",
                "diff",
                "--name-only",
                "--diff-filter=A",
                "--no-renames",
                "base...head",
            ],
            capture_output=True,
            text=True,
            check=True,
        )

    def drops_blank_lines():
        runner = mock.Mock(return_value=mock.Mock(stdout="a.py\n\n"))
        assert diff_names("base", "head", runner=runner) == ["a.py"]
