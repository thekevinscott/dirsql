"""Assert the precheck build matrix actually produced every artifact (#790).

`release-precheck.yml` runs the same build matrix as the release, but has no
publish job -- so it never reaches the artifact-completeness check that publish
performs. `actions/upload-artifact` only *warns* when its path matches nothing,
so a build row that stages into the wrong directory uploads nothing and still
reports success. #776, #777 and #778 were each green that way while leaving
`main` unpublishable; the failure surfaced only after merge, at release.
"""

from __future__ import annotations

import sys


def declared_targets(config: dict) -> list[tuple[str, str]]:
    """(package name, target triple) for every package that declares targets."""
    pairs = []
    for package in config.get("package", []):
        name = package.get("name")
        for target in package.get("targets", []):
            triple = target if isinstance(target, str) else target.get("triple")
            if name and triple:
                pairs.append((name, triple))
    return pairs


def warn(line: str) -> None:
    print(line, file=sys.stderr)
