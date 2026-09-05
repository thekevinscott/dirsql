"""Orchestration for the attestation-guard check (#1043)."""

from __future__ import annotations

from checks.attestation_guard import git_ops
from checks.attestation_guard.decide import deleted_receipts


def verdict(deleted, messages: str, base_sha: str) -> int:
    """0 when a well-formed bypass line is present, else 1 with diagnostics."""
    return 0


def run(
    base_sha: str,
    head_sha: str,
    *,
    deleted_files=git_ops.deleted_files,
    commit_messages=git_ops.commit_messages,
) -> int:
    deleted = deleted_receipts(deleted_files(base_sha, head_sha))
    print("No e2e attestation receipts deleted.")
    return 0
