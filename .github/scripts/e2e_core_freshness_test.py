"""Unit tests for the shared-core e2e-attestation freshness gate (#337).

Mocks every collaborator: ``subprocess.run`` (the git calls), and, for
``main``, the module's own helpers plus the attestation file read -- so each
test exercises only its unit. Mirrors the repo's pytest-describe style.
"""

from unittest.mock import MagicMock, mock_open, patch

import e2e_core_freshness as fresh


def describe_latest_core_commit():
    def it_returns_the_sha_when_a_binding_linked_core_commit_exists():
        with patch.object(
            fresh.subprocess, "run", return_value=MagicMock(stdout="abc123\n")
        ) as run:
            assert fresh.latest_core_commit("BASE", "HEAD") == "abc123"
        argv = run.call_args.args[0]
        # two-dot range (the PR's own commits since merge-base), newest only --
        # three-dot rev-list would be the symmetric diff and pick up base-side
        # core commits the PR never made
        assert argv[:4] == ["git", "rev-list", "-1", "BASE..HEAD"]
        # cli-gated core is excluded from the staling set
        assert ":(exclude)packages/rust/src/cli" in argv
        assert ":(exclude)packages/rust/src/bin" in argv
        assert "packages/rust/src" in argv

    def it_returns_none_when_no_binding_linked_core_changed():
        with patch.object(
            fresh.subprocess, "run", return_value=MagicMock(stdout="  \n")
        ):
            assert fresh.latest_core_commit("BASE", "HEAD") is None


def describe_attestation_commit():
    def it_reads_the_commit_field_from_the_binding_attestation():
        with patch(
            "builtins.open", mock_open(read_data='{"commit": "deadbeef"}')
        ) as opened:
            assert fresh.attestation_commit("python") == "deadbeef"
        assert opened.call_args.args[0] == "packages/python/e2e-attestation.json"


def describe_includes():
    def it_is_true_when_merge_base_is_ancestor_exits_zero():
        with patch.object(
            fresh.subprocess, "run", return_value=MagicMock(returncode=0)
        ) as run:
            assert fresh.includes("CORE", "ATTEST") is True
        assert run.call_args.args[0] == [
            "git",
            "merge-base",
            "--is-ancestor",
            "CORE",
            "ATTEST",
        ]

    def it_is_false_when_merge_base_is_ancestor_exits_nonzero():
        with patch.object(
            fresh.subprocess, "run", return_value=MagicMock(returncode=1)
        ):
            assert fresh.includes("CORE", "ATTEST") is False


def describe_main():
    def it_returns_zero_when_no_binding_linked_core_changed():
        with patch.object(fresh, "latest_core_commit", return_value=None):
            assert fresh.main(["prog", "BASE", "HEAD"]) == 0

    def it_returns_zero_when_both_attestations_include_the_core_commit():
        with (
            patch.object(fresh, "latest_core_commit", return_value="CORE"),
            patch.object(fresh, "attestation_commit", side_effect=["PY", "TS"]),
            patch.object(fresh, "includes", return_value=True),
        ):
            assert fresh.main(["prog", "BASE", "HEAD"]) == 0

    def it_returns_one_when_a_binding_attestation_is_stale(capsys):
        with (
            patch.object(fresh, "latest_core_commit", return_value="CORE"),
            patch.object(fresh, "attestation_commit", side_effect=["PY", "TS"]),
            # python includes the core commit, ts does not
            patch.object(fresh, "includes", side_effect=[True, False]),
        ):
            assert fresh.main(["prog", "BASE", "HEAD"]) == 1
        err = capsys.readouterr().err
        assert "stale: ts" in err
