"""Orchestration for the attestation-guard check (#1043).

Fails a PR whose `base...HEAD` diff deletes any e2e attestation receipt. The
git plumbing is injected so the orchestration unit-tests without a real repo.
"""

from __future__ import annotations

from checks.attestation_guard import git_ops
from checks.attestation_guard.decide import (
    deleted_receipts,
    has_allow_line,
    near_miss_lines,
)
from checks.attestation_guard.report import report


def verdict(deleted, messages: str, base_sha: str) -> int:
    """0 when a well-formed bypass line is present, else 1 with diagnostics."""
    if has_allow_line(messages):
        print("allow-receipt-deletion line present; permitting receipt deletion.")
        return 0
    report(deleted, near_miss_lines(messages), base_sha)
    return 1


def run(
    base_sha: str,
    head_sha: str,
    *,
    deleted_files=git_ops.deleted_files,
    commit_messages=git_ops.commit_messages,
) -> int:
    deleted = deleted_receipts(deleted_files(base_sha, head_sha))
    if not deleted:
        print("No e2e attestation receipts deleted.")
        return 0
    return verdict(deleted, commit_messages(base_sha, head_sha), base_sha)
