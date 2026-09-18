"""The paths a range deletes, for the attestation-guard check (#1043)."""

from __future__ import annotations

import subprocess

from ..git.diff_names import diff_names


def deleted_files(base_sha: str, head_sha: str, runner=subprocess.run) -> list[str]:
    # `--no-renames` so a receipt moved to another path registers as a deletion
    # of the original: a receipt's path is its branch slug, so renaming one is
    # as much a loss of the record as removing it.
    return diff_names(base_sha, head_sha, ("--diff-filter=D", "--no-renames"), runner)
