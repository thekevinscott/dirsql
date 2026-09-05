"""The commit bodies over a range, for the attestation-guard check (#1043).

Shells out for exactly this one piece of data, so the decision logic in
`decide.py` stays subprocess-free and unit-testable.
"""

from __future__ import annotations

import subprocess


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
