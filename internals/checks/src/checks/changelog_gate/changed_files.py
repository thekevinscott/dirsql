"""The PR's changed-file list, straight from git (#494/#496).

The decision logic in `decide.py` never touches a subprocess, so it stays
unit-testable without mocking; each git query lives in its own module beside
this one.
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
