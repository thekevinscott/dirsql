"""Thin git subprocess wrappers for the attestation-guard check (#1043).

Each shells out for exactly one piece of data, so the decision logic in
`decide.py` stays subprocess-free and unit-testable.
"""

from __future__ import annotations

import subprocess


def deleted_files(base_sha: str, head_sha: str, runner=subprocess.run) -> list[str]:
    # `--no-renames` so a receipt moved to another path registers as a deletion
    # of the original: a receipt's path is its branch slug, so renaming one is
    # as much a loss of the record as removing it.
    result = runner(
        ["git", "diff", "--name-only", "--diff-filter=D", "--no-renames",
         f"{base_sha}...{head_sha}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return [line for line in result.stdout.splitlines() if line]


def commit_messages(base_sha: str, head_sha: str, runner=subprocess.run) -> str:
    # Raw bodies (`%B`), scanned for the bypass line without git's trailer
    # parser, so the bypass works from any line of any commit in the range.
    result = runner(
        ["git", "log", "--format=%B", f"{base_sha}..{head_sha}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout
