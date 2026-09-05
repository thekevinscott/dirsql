from unittest import mock

import pytest

from checks.attestation_guard.gate import run, verdict


def _run(deleted, *, messages="fix: a thing\n"):
    return run(
        "base",
        "head",
        deleted_files=mock.Mock(return_value=deleted),
        commit_messages=mock.Mock(return_value=messages),
    )


def describe_run():
    def passes_when_the_diff_deletes_nothing(capsys):
        assert _run([]) == 0
        assert "No e2e attestation receipts deleted." in capsys.readouterr().out

    def passes_when_the_diff_deletes_no_receipt(capsys):
        assert _run(["packages/python/dirsql/core.py"]) == 0
        assert "No e2e attestation receipts deleted." in capsys.readouterr().out

    def does_not_read_commit_messages_when_nothing_was_deleted():
        messages = mock.Mock()
        run(
            "base",
            "head",
            deleted_files=mock.Mock(return_value=[]),
            commit_messages=messages,
        )
        messages.assert_not_called()

    def fails_when_the_diff_deletes_a_receipt(capsys):
        assert _run(["packages/ts/e2e-attestations/other-branch.json"]) == 1
        assert "other-branch.json is an e2e attestation receipt" in capsys.readouterr().out

    def names_the_restore_command_with_the_base_sha(capsys):
        assert _run(["packages/ts/e2e-attestations/a.json"]) == 1
        out = capsys.readouterr().out
        assert "git checkout base -- packages/ts/e2e-attestations/a.json" in out

    def passes_when_a_bypass_line_is_present(capsys):
        rc = _run(
            ["packages/ts/e2e-attestations/a.json"],
            messages="chore: retire pkg\n\nallow-receipt-deletion: package removed",
        )
        assert rc == 0
        assert "permitting receipt deletion" in capsys.readouterr().out

    def fails_and_names_a_near_miss_bypass(capsys):
        rc = _run(
            ["packages/ts/e2e-attestations/a.json"],
            messages="chore: retire pkg\n\nskip-receipt-deletion: package removed",
        )
        assert rc == 1
        assert "is not the bypass line" in capsys.readouterr().out

    def keeps_the_injected_collaborators_keyword_only():
        # The `*` in the signature is the seam's contract: a positional third
        # argument must not silently bind to `deleted_files`.
        with pytest.raises(TypeError):
            run("base", "head", mock.Mock(return_value=[]), mock.Mock())

    def queries_git_with_the_supplied_shas():
        deleted = mock.Mock(return_value=[])
        run("b1", "h1", deleted_files=deleted, commit_messages=mock.Mock())
        deleted.assert_called_once_with("b1", "h1")

    def reads_commit_messages_over_the_same_range():
        messages = mock.Mock(return_value="fix: x\n")
        run(
            "b1",
            "h1",
            deleted_files=mock.Mock(return_value=["a/e2e-attestations/x.json"]),
            commit_messages=messages,
        )
        messages.assert_called_once_with("b1", "h1")


def describe_verdict():
    def reports_every_deleted_receipt(capsys):
        rc = verdict(
            ["a/e2e-attestations/x.json", "b/e2e-attestations/y.json"], "fix: x", "sha"
        )
        assert rc == 1
        assert capsys.readouterr().out.count("::error file=") == 2

    def does_not_report_when_bypassed(capsys):
        rc = verdict(["a/e2e-attestations/x.json"], "allow-receipt-deletion: why", "sha")
        assert rc == 0
        assert "::error file=" not in capsys.readouterr().out
