from unittest import mock

from checks.changelog_gate.commit_messages import commit_messages


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
