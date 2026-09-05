"""Failure output for the attestation-guard check (#1043)."""

from __future__ import annotations


def restore_command(deleted, base_sha: str) -> str:
    """The exact `git checkout` that puts every deleted receipt back."""
    return ""


def report(deleted, near_misses, base_sha: str) -> None:
    """Print one annotation per deleted receipt, then how to undo them."""
