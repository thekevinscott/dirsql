"""Orchestration for the attestation-guard check (#1043).

Fails a PR whose `base...HEAD` diff deletes any e2e attestation receipt. The
git plumbing is injected so the orchestration unit-tests without a real repo.
"""

from __future__ import annotations

from checks.attestation_guard.commit_messages import commit_messages
from checks.attestation_guard.decide import deleted_receipts
from checks.attestation_guard.deleted_files import deleted_files
from checks.attestation_guard.verdict import verdict


def run(
    base_sha: str,
    head_sha: str,
    *,
    deleted_files=deleted_files,
    commit_messages=commit_messages,
) -> int:
    deleted = deleted_receipts(deleted_files(base_sha, head_sha))
    if not deleted:
        print("No e2e attestation receipts deleted.")
        return 0
    return verdict(deleted, commit_messages(base_sha, head_sha), base_sha)
