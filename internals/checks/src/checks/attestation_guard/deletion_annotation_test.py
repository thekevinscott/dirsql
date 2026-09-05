from checks.attestation_guard.deletion_annotation import deletion_annotation


def describe_deletion_annotation():
    def names_the_file_and_the_append_only_rule():
        line = deletion_annotation("packages/ts/e2e-attestations/a.json")
        assert line.startswith("::error file=packages/ts/e2e-attestations/a.json::")
        assert "packages/ts/e2e-attestations/a.json is an e2e attestation receipt" in line
        assert "Receipts are append-only; this PR deletes it." in line
