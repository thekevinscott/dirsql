"""Thin git subprocess wrappers for the changelog-gate check (#494/#496).

Each function shells out for exactly one piece of data and returns raw text/lines; the decision
logic in `decide.py` never touches a subprocess, so it stays unit-testable without mocking.
"""
from __future__ import annotations

import subprocess


def changed_files(base_sha: str, head_sha: str, runner=subprocess.run) -> list[str]:
    # Three-dot (BASE...HEAD) diffs from the merge-base, so it lists only the files this PR's
    # own commits changed -- not files main changed after this branch forked.
    result = runner(
        ["git", "diff", "--name-only", f"{base_sha}...{head_sha}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return [line for line in result.stdout.splitlines() if line]


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


def commit_messages(base_sha: str, head_sha: str, runner=subprocess.run) -> str:
    # Raw commit bodies (`%B`) for every commit in the range, scanned for a
    # `skip-changelog:` line -- git's own trailer parser is bypassed so the
    # bypass works from any line, not only a formal final-paragraph trailer.
    result = runner(
        ["git", "log", "--format=%B", f"{base_sha}..{head_sha}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout
