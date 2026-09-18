"""The files the PR adds, straight from git (#494/#496)."""
from __future__ import annotations

import subprocess

from ..git.diff_names import diff_names


def added_files(base_sha: str, head_sha: str, runner=subprocess.run) -> list[str]:
    # A fragment satisfies the gate only when the PR *adds* it, so an edit to an
    # existing fragment never counts.
    return diff_names(base_sha, head_sha, ("--diff-filter=A",), runner)
