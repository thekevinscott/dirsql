"""The attestation-guard verdict once a receipt deletion is known (#1043).

Split from the orchestration so the bypass decision reads on its own: the
deletion is already established here, and all that remains is whether a
well-formed bypass line excuses it.
"""

from __future__ import annotations

from checks.attestation_guard.decide import has_allow_line, near_miss_lines
from checks.attestation_guard.report import report


def verdict(deleted, messages: str, base_sha: str) -> int:
    """0 when a well-formed bypass line is present, else 1 with diagnostics."""
    if has_allow_line(messages):
        print("allow-receipt-deletion line present; permitting receipt deletion.")
        return 0
    report(deleted, near_miss_lines(messages), base_sha)
    return 1
