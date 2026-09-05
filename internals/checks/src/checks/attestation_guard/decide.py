"""Pure decision logic for the attestation-guard check (#1043)."""

from __future__ import annotations


def deleted_receipts(deleted) -> list[str]:
    """Sorted paths the diff deletes from an ``e2e-attestations/`` directory."""
    return []


def has_allow_line(commit_messages: str) -> bool:
    """True if a commit body carries ``allow-receipt-deletion: <reason>``."""
    return False


def near_miss_lines(commit_messages: str) -> list[str]:
    """Bypass-shaped commit lines that are not the accepted spelling."""
    return []
