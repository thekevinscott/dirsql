"""Names of the files a `base...head` range touches, straight from git."""

from __future__ import annotations

import subprocess


def diff_names(
    base_sha: str,
    head_sha: str,
    flags: tuple[str, ...] = (),
    runner=subprocess.run,
) -> list[str]:
    # Three-dot (BASE...HEAD) diffs from the merge-base, so the list covers only
    # what this branch's own commits changed, not what main changed after it forked.
    result = runner(
        ["git", "diff", "--name-only", *flags, f"{base_sha}...{head_sha}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return [line for line in result.stdout.splitlines() if line]
