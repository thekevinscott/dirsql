"""Pure decision logic for the attestation-guard check (#1043).

E2E attestation receipts are append-only by convention
(``agents/reference/e2e-attestation.md``): a merged branch's receipt stays in
the directory as the record that its suite ran. The rule here is therefore
**total** -- any deletion under an ``e2e-attestations/`` directory fails, with
no carve-out for "the branch's own" receipt. A branch that re-attests
*modifies* its receipt; it never deletes one. Slug-keyed ownership would also
mis-handle the real case from #1036, where a merged child branch's receipt
rode in under a different slug and looked foreign.

An ``allow-receipt-deletion: <reason>`` commit-body line is the bypass,
mirroring changelog-gate's ``skip-changelog:``. The reason is mandatory so a
bare word cannot disarm the gate by accident.
"""

from __future__ import annotations

import re

# Any file inside a directory named `e2e-attestations`, at any depth, so a new
# package's receipt directory is covered the day it is created.
_RECEIPT = re.compile(r"(?:.+/)?e2e-attestations/.+")

# Scanned over raw commit bodies rather than git's trailer parser, matching
# changelog-gate: a blank line must not split the bypass off the message.
_ALLOW = re.compile(r"(?im)^[ \t]*allow-receipt-deletion:[ \t]*\S.*$")

# Deliberately loose, so a bypass that was attempted but misspelled (wrong
# verb, wrong noun, pluralised, reason omitted) is named instead of ignored.
_NEAR = re.compile(
    r"(?im)^[ \t]*(?:skip|allow)[-_ ]?(?:receipt|attestation)s?[-_ ]?deletions?[ \t]*:?.*$"
)


def deleted_receipts(deleted) -> list[str]:
    """Sorted paths the diff deletes from an ``e2e-attestations/`` directory."""
    return sorted(path for path in deleted if _RECEIPT.fullmatch(path))


def has_allow_line(commit_messages: str) -> bool:
    """True if a commit body carries ``allow-receipt-deletion: <reason>``."""
    return bool(_ALLOW.search(commit_messages))


def near_miss_lines(commit_messages: str) -> list[str]:
    """Bypass-shaped commit lines that are not the accepted spelling."""
    return [
        line.strip()
        for line in _NEAR.findall(commit_messages)
        if not _ALLOW.fullmatch(line)
    ]
