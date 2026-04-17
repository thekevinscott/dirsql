"""Compute the next release version from the latest git tag.

Consumed by the `tag` job in .github/workflows/publish.yml.
"""

from __future__ import annotations

import os
import re
import sys
from dataclasses import dataclass

_TAG_RE = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
_VALID_BUMPS = ("patch", "minor")


@dataclass(frozen=True)
class Decision:
    new_version: str
    should_release: bool


def compute(*, latest_tag: str, bump_type: str, commits_since_tag: int) -> Decision:
    if bump_type not in _VALID_BUMPS:
        raise ValueError(f"unknown bump type: {bump_type!r}")
    if commits_since_tag < 0:
        raise ValueError(f"commits_since_tag must be >= 0, got {commits_since_tag}")

    if not latest_tag:
        # No prior tag -- bump from 0.0.0. Always release.
        major, minor, patch = 0, 0, 0
        has_tag = False
    else:
        m = _TAG_RE.match(latest_tag)
        if not m:
            raise ValueError(f"malformed tag: {latest_tag!r}")
        major, minor, patch = (int(x) for x in m.groups())
        has_tag = True

    if bump_type == "patch":
        new_version = f"{major}.{minor}.{patch + 1}"
        # Skip scheduled/push patch releases that have no new commits.
        should_release = not (has_tag and commits_since_tag == 0)
    else:  # minor
        new_version = f"{major}.{minor + 1}.0"
        should_release = True

    return Decision(new_version=new_version, should_release=should_release)


def main() -> int:
    try:
        commits = int(os.environ.get("COMMITS_SINCE_TAG", "0"))
    except ValueError:
        commits = 0
    decision = compute(
        latest_tag=os.environ.get("LATEST_TAG", ""),
        bump_type=os.environ.get("BUMP_TYPE", "patch"),
        commits_since_tag=commits,
    )
    lines = [
        f"new_version={decision.new_version}",
        f"should_release={'true' if decision.should_release else 'false'}",
    ]
    text = "\n".join(lines) + "\n"
    out_path = os.environ.get("GITHUB_OUTPUT")
    if out_path:
        with open(out_path, "a", encoding="utf-8") as fh:
            fh.write(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
