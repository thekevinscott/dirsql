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
    return result.stdout.splitlines()


def skip_trailers(base_sha: str, head_sha: str, runner=subprocess.run) -> str:
    result = runner(
        [
            "git",
            "log",
            "--format=%(trailers:key=skip-changelog,valueonly)",
            f"{base_sha}..{head_sha}",
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout


def commit_messages(base_sha: str, head_sha: str, runner=subprocess.run) -> str:
    # Raw commit bodies (`%B`) for every commit in the range, so the gate can
    # detect a `skip-changelog:` line that git did NOT parse as a trailer (e.g.
    # split out of the final trailer block by a blank line) and report it,
    # instead of falling through to the generic "no changelog entry" message.
    result = runner(
        ["git", "log", "--format=%B", f"{base_sha}..{head_sha}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout
