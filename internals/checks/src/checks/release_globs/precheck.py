"""Whether `release-ci.yml`'s path filter skips only what publishing also skips (#944)."""

from __future__ import annotations

from .decide import NON_SHIPPING_DIRS, NON_SHIPPING_FILES
from .subtree_root import subtree_root


def unprechecked(exclusions) -> list[str]:
    """One message per precheck exclusion the publish globs would still match.

    `release-ci.yml` is the PR-time build precheck; every path it skips must be
    a path publishing also skips, or a merge cuts a release the matrix never
    built. The comparison is by name rather than by matching, so it needs no
    second copy of putitoutthere's glob semantics.
    """
    problems = []
    for entry in exclusions:
        path = entry.removeprefix("!")
        root = subtree_root(path)
        if root is None:
            continue
        rest = path[len(root) + 1 :]
        if rest.endswith("/**") and rest.removesuffix("/**") in NON_SHIPPING_DIRS:
            continue
        if rest in NON_SHIPPING_FILES:
            continue
        problems.append(
            f'release-ci.yml excludes "{entry}", but publishing does not: '
            f"{rest.removesuffix('/**')} is not one of "
            f"{', '.join((*NON_SHIPPING_DIRS, *NON_SHIPPING_FILES))}, so "
            f"putitoutthere still cascades {root} on a change there and a merge "
            f"releases what the precheck never built. Either drop the exclusion "
            f"from release-ci.yml or add the name to NON_SHIPPING_DIRS / "
            f"NON_SHIPPING_FILES and re-run this check."
        )
    return problems
