from checks.attestation_guard.verdict import verdict


def describe_verdict():
    def reports_every_deleted_receipt(capsys):
        rc = verdict(
            ["a/e2e-attestations/x.json", "b/e2e-attestations/y.json"], "fix: x", "sha"
        )
        assert rc == 1
        assert capsys.readouterr().out.count("::error file=") == 2

    def names_the_restore_command_with_the_base_sha(capsys):
        assert verdict(["a/e2e-attestations/x.json"], "fix: x", "abc123") == 1
        out = capsys.readouterr().out
        assert "git checkout abc123 -- a/e2e-attestations/x.json" in out

    def passes_and_says_so_when_a_bypass_line_is_present(capsys):
        rc = verdict(["a/e2e-attestations/x.json"], "allow-receipt-deletion: why", "sha")
        assert rc == 0
        assert "permitting receipt deletion" in capsys.readouterr().out

    def does_not_report_when_bypassed(capsys):
        rc = verdict(["a/e2e-attestations/x.json"], "allow-receipt-deletion: why", "sha")
        assert rc == 0
        assert "::error file=" not in capsys.readouterr().out

    def names_a_near_miss_bypass_line(capsys):
        rc = verdict(["a/e2e-attestations/x.json"], "skip-receipt-deletion: y", "sha")
        assert rc == 1
        out = capsys.readouterr().out
        assert "'skip-receipt-deletion: y' is not the bypass line" in out

    def reports_no_near_miss_when_the_messages_hold_none(capsys):
        assert verdict(["a/e2e-attestations/x.json"], "fix: x", "sha") == 1
        assert "is not the bypass line" not in capsys.readouterr().out
