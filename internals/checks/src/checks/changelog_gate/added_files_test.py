from unittest import mock

from checks.changelog_gate.added_files import added_files


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

    def drops_blank_lines():
        runner = mock.Mock(return_value=mock.Mock(stdout="new.md\n\n"))
        assert added_files("base", "head", runner=runner) == ["new.md"]
