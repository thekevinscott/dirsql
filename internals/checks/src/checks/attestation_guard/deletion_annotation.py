"""The per-file annotation for one wrongly-deleted receipt (#1043)."""

from __future__ import annotations


def deletion_annotation(path: str) -> str:
    """The per-file annotation naming one wrongly-deleted receipt."""
    return (
        f"::error file={path}::{path} is an e2e attestation receipt. "
        "Receipts are append-only; this PR deletes it."
    )
