"""Failure output for the attestation-guard check (#1043).

AGENTS.md requires every CI check to emit actionable fix instructions, so this
names each wrongly-deleted receipt and prints the exact `git checkout` that
restores it, plus a near-miss diagnostic when a bypass line was attempted but
misspelled.
"""

from __future__ import annotations

from checks.attestation_guard.deletion_annotation import deletion_annotation
from checks.attestation_guard.near_miss_annotation import near_miss_annotation

_ADVICE = (
    "::error::A receipt records that another branch's e2e suite ran, and "
    "nothing can regenerate a merged branch's. Restore them and re-attest only "
    "your own branch. If the deletion is deliberate (retiring a package), add "
    "an 'allow-receipt-deletion: <reason>' line to any commit message."
)


def restore_command(deleted, base_sha: str) -> str:
    """The exact `git checkout` that puts every deleted receipt back."""
    return f"git checkout {base_sha} -- {' '.join(deleted)}"


def report(deleted, near_misses, base_sha: str) -> None:
    """Print one annotation per deleted receipt, then how to undo them."""
    for path in deleted:
        print(deletion_annotation(path))
    for line in near_misses:
        print(near_miss_annotation(line))
    print(
        f"::error::Restore {len(deleted)} deleted receipt(s) with: "
        f"{restore_command(deleted, base_sha)}"
    )
    print(_ADVICE)
