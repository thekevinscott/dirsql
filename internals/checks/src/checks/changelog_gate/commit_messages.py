"""The PR's raw commit bodies, straight from git (#494/#496)."""
from __future__ import annotations

import subprocess


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
