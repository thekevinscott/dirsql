"""The raw commit bodies over a range, straight from git."""

from __future__ import annotations

import subprocess


def commit_messages(base_sha: str, head_sha: str, runner=subprocess.run) -> str:
    # Raw bodies (`%B`), so a bypass line is found anywhere in a commit message
    # rather than only in git's own trailer block.
    result = runner(
        ["git", "log", "--format=%B", f"{base_sha}..{head_sha}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout
