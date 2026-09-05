"""The files the PR adds, straight from git (#494/#496)."""
from __future__ import annotations

import subprocess


def added_files(base_sha: str, head_sha: str, runner=subprocess.run) -> list[str]:
    # Added-only (`--diff-filter=A`) over the same merge-base range: a fragment satisfies the
    # gate only when the PR *adds* it, so an edit to an existing fragment never counts.
    result = runner(
        ["git", "diff", "--name-only", "--diff-filter=A", f"{base_sha}...{head_sha}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return [line for line in result.stdout.splitlines() if line]
