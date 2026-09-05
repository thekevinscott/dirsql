from unittest import mock

from checks.changelog_gate.changed_files import changed_files


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
