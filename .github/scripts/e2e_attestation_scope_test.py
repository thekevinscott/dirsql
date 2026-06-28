"""Unit tests for the e2e-attestation scope script.

Mocks every collaborator: ``subprocess.run`` (the git call) for
``package_changed``, and ``package_changed`` + ``os.environ`` for ``main`` --
so each test exercises only its unit. Mirrors the repo's pytest-describe
style.
"""

from unittest.mock import MagicMock, patch

import e2e_attestation_scope as scope


def describe_package_changed():
    def it_is_true_when_the_diff_lists_a_non_attestation_file():
        with patch.object(
            scope.subprocess,
            "run",
            return_value=MagicMock(stdout="packages/python/dirsql/x.py\n"),
        ) as run:
            assert scope.package_changed("python", "BASE", "HEAD") is True
        argv = run.call_args.args[0]
        # three-dot diff (merge-base..head) so main's post-branch commits
        # don't masquerade as this PR's changes
        assert argv[:4] == ["git", "diff", "--name-only", "BASE...HEAD"]
        # the package's own attestation is excluded from the diff
        assert ":(exclude)packages/python/e2e-attestation.json" in argv

    def it_is_false_when_the_diff_is_empty():
        with patch.object(
            scope.subprocess, "run", return_value=MagicMock(stdout="  \n")
        ):
            assert scope.package_changed("ts", "BASE", "HEAD") is False


def describe_main():
    def it_writes_booleans_to_github_output_and_returns_zero(tmp_path):
        out = tmp_path / "out.txt"
        with (
            patch.object(scope, "package_changed", side_effect=[True, False]),
            patch.dict(scope.os.environ, {"GITHUB_OUTPUT": str(out)}, clear=True),
        ):
            assert scope.main(["prog", "BASE", "HEAD"]) == 0
        assert out.read_text().splitlines() == ["python=true", "ts=false"]

    def it_skips_the_output_file_when_github_output_is_unset(capsys):
        with (
            patch.object(scope, "package_changed", side_effect=[False, True]),
            patch.dict(scope.os.environ, {}, clear=True),
        ):
            assert scope.main(["prog", "BASE", "HEAD"]) == 0
        printed = capsys.readouterr().out
        assert "python package changed: no" in printed
        assert "ts package changed: yes" in printed
