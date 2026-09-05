from checks.attestation_guard.report import (
    deletion_annotation,
    near_miss_annotation,
    report,
    restore_command,
)


def describe_restore_command():
    def names_the_base_sha_and_every_path():
        assert restore_command(["a.json", "b.json"], "abc123") == (
            "git checkout abc123 -- a.json b.json"
        )


def describe_deletion_annotation():
    def names_the_file_and_the_append_only_rule():
        line = deletion_annotation("packages/ts/e2e-attestations/a.json")
        assert line.startswith("::error file=packages/ts/e2e-attestations/a.json::")
        assert "packages/ts/e2e-attestations/a.json is an e2e attestation receipt" in line
        assert "Receipts are append-only; this PR deletes it." in line


def describe_near_miss_annotation():
    def quotes_the_line_and_gives_the_exact_form():
        line = near_miss_annotation("skip-receipt-deletion: y")
        assert line == (
            "::error::'skip-receipt-deletion: y' is not the bypass line. "
            "The exact form is 'allow-receipt-deletion: <reason>', reason required."
        )


def describe_report():
    def annotates_every_deleted_receipt_with_its_own_file(capsys):
        report(["packages/ts/e2e-attestations/a.json"], [], "abc123")
        out = capsys.readouterr().out
        assert "::error file=packages/ts/e2e-attestations/a.json::" in out
        assert "append-only" in out

    def prints_one_annotation_per_deleted_receipt(capsys):
        report(["a/e2e-attestations/x.json", "b/e2e-attestations/y.json"], [], "abc")
        out = capsys.readouterr().out
        assert out.count("::error file=") == 2

    def prints_the_exact_restore_command(capsys):
        report(["a/e2e-attestations/x.json"], [], "abc123")
        assert "git checkout abc123 -- a/e2e-attestations/x.json" in capsys.readouterr().out

    def counts_the_deleted_receipts(capsys):
        report(["a/e2e-attestations/x.json", "a/e2e-attestations/y.json"], [], "abc")
        assert "Restore 2 deleted receipt(s)" in capsys.readouterr().out

    def names_the_bypass_line_in_the_closing_advice(capsys):
        report(["a/e2e-attestations/x.json"], [], "abc")
        assert "allow-receipt-deletion: <reason>" in capsys.readouterr().out

    def quotes_a_near_miss_and_gives_the_exact_form(capsys):
        report(["a/e2e-attestations/x.json"], ["skip-receipt-deletion: y"], "abc")
        out = capsys.readouterr().out
        assert "'skip-receipt-deletion: y' is not the bypass line" in out
        assert "reason required" in out

    def prints_one_annotation_per_near_miss(capsys):
        report(["a/e2e-attestations/x.json"], ["one:", "two:"], "abc")
        assert capsys.readouterr().out.count("is not the bypass line") == 2

    def prints_no_near_miss_annotation_when_there_is_none(capsys):
        report(["a/e2e-attestations/x.json"], [], "abc")
        assert "is not the bypass line" not in capsys.readouterr().out
