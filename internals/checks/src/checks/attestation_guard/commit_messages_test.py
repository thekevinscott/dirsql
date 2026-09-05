from unittest import mock

from checks.attestation_guard.commit_messages import commit_messages


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
