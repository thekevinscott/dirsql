from checks.attestation_guard.decide import (
    deleted_receipts,
    has_allow_line,
    near_miss_lines,
)


def describe_deleted_receipts():
    def keeps_only_paths_under_an_e2e_attestations_directory():
        assert deleted_receipts(
            [
                "packages/python/e2e-attestations/claude-a.json",
                "packages/python/dirsql/core.py",
                "README.md",
            ]
        ) == ["packages/python/e2e-attestations/claude-a.json"]

    def spans_every_package_root_that_holds_receipts():
        paths = [
            "internals/checks/e2e-attestations/a.json",
            "plugins/dirsql-plugin-embeddings/e2e-attestations/b.json",
            "packages/ts/e2e-attestations/c.json",
        ]
        assert deleted_receipts(paths) == sorted(paths)

    def sorts_the_result():
        assert deleted_receipts(
            ["packages/ts/e2e-attestations/b.json", "packages/ts/e2e-attestations/a.json"]
        ) == [
            "packages/ts/e2e-attestations/a.json",
            "packages/ts/e2e-attestations/b.json",
        ]

    def ignores_the_directory_entry_itself():
        assert deleted_receipts(["packages/ts/e2e-attestations/"]) == []

    def ignores_a_lookalike_directory_name():
        assert deleted_receipts(["packages/ts/e2e-attestations-old/a.json"]) == []

    def returns_nothing_for_an_empty_diff():
        assert deleted_receipts([]) == []


def describe_has_allow_line():
    def accepts_the_bypass_with_a_reason():
        assert has_allow_line("chore: x\n\nallow-receipt-deletion: retiring pkg") is True

    def accepts_it_regardless_of_case():
        assert has_allow_line("Allow-Receipt-Deletion: retiring pkg") is True

    def accepts_it_from_any_line_not_only_a_formal_trailer():
        body = "allow-receipt-deletion: retiring pkg\n\nchore: x\n"
        assert has_allow_line(body) is True

    def rejects_the_bypass_with_no_reason():
        assert has_allow_line("chore: x\n\nallow-receipt-deletion:") is False

    def rejects_the_bypass_with_only_whitespace_for_a_reason():
        assert has_allow_line("chore: x\n\nallow-receipt-deletion:   ") is False

    def rejects_an_ordinary_message():
        assert has_allow_line("fix: delete a stale thing\n") is False


def describe_near_miss_lines():
    def names_a_bypass_spelled_with_the_wrong_verb():
        assert near_miss_lines("skip-receipt-deletion: cleanup") == [
            "skip-receipt-deletion: cleanup"
        ]

    def names_a_bypass_spelled_with_the_wrong_noun():
        assert near_miss_lines("allow-attestation-deletion: cleanup") == [
            "allow-attestation-deletion: cleanup"
        ]

    def names_a_bypass_missing_its_reason():
        assert near_miss_lines("allow-receipt-deletion:") == ["allow-receipt-deletion:"]

    def names_a_bypass_missing_its_colon():
        assert near_miss_lines("allow-receipt-deletion cleanup") == [
            "allow-receipt-deletion cleanup"
        ]

    def does_not_name_the_accepted_spelling():
        assert near_miss_lines("allow-receipt-deletion: retiring pkg") == []

    def strips_surrounding_whitespace():
        assert near_miss_lines("   skip-receipt-deletion:  ") == ["skip-receipt-deletion:"]

    def returns_nothing_for_an_ordinary_message():
        assert near_miss_lines("fix: a thing\n") == []
