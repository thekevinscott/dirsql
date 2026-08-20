"""Pure decision logic for the release-globs check (#944).

Merging to `main` is the release trigger, and putitoutthere decides what to
release by intersecting each `[[package]]`'s `globs` with that package's diff
since its own last tag. A bare ``packages/<pkg>/**`` therefore republishes on a
changelog fragment, an e2e attestation receipt, or a `testing-conventions.toml`
edit -- none of which reach the built artifact. Run 371 did exactly that: two
new gate-config files republished `dirsql-py` and `dirsql-npm` byte-identical to
the release before them.

**putitoutthere has no glob exclusion field and its leading-`!` negations do the
opposite of what they look like.** Verified against the pinned `@v0` sha
8f5876751f679ee3450617957c97495205002499: `globs` is `z.array(z.string())` on a
`.strict()` schema (no `exclude`-shaped key is representable), and both callers
-- `cascade.ts`'s seed pass and `check.ts` -- go through `glob.ts`'s
``matchesAny``, a plain OR that returns on the first hit. Under minimatch
10.2.6 a `!`-prefixed pattern matches every path *outside* its own subtree, so
adding ``!packages/rust/changelog.d/**`` neither subtracts the fragment (the
earlier ``packages/rust/**`` already returned true) nor stays inert -- it makes
`README.md` match, and the package cascades on every commit.

The form that does work is minimatch's extglob, which excludes *inside* a single
positive pattern: ``packages/rust/!(changelog.d|migrations.d)/**``. It stays a
carve-out rather than an allowlist -- a subdirectory added later is covered
automatically, and only the named names are dropped.
"""

from __future__ import annotations

# Non-shipping paths inside a published package's tree: the changelog and
# migration fragment dirs, the e2e attestation receipts, and the package's own
# testing-conventions gate config.
NON_SHIPPING_DIRS = ("changelog.d", "migrations.d", "e2e-attestations")
NON_SHIPPING_FILES = ("testing-conventions.toml",)

# Top-level directories holding independently published packages.
PUBLISHED_ROOTS = ("packages", "plugins")

_GLOB_META = "*?[]{}!"


def carve_out(root: str) -> list[str]:
    """The canonical publish globs for the package subtree at ``root``.

    Two patterns, because a single extglob cannot cover both depths: the first
    matches files sitting directly in the package root, the second everything
    under its subdirectories.
    """
    return [
        f"{root}/!({'|'.join(NON_SHIPPING_FILES)})",
        f"{root}/!({'|'.join(NON_SHIPPING_DIRS)})/**",
    ]


def subtree_root(glob: str):
    """``<root>/<package>`` when ``glob`` reaches into a published package tree,
    else ``None``."""
    parts = glob.split("/")
    if len(parts) < 3 or parts[0] not in PUBLISHED_ROOTS:
        return None
    return f"{parts[0]}/{parts[1]}"


def is_wildcard(glob: str) -> bool:
    """True when the entry is a pattern rather than a literal path."""
    return any(character in glob for character in _GLOB_META)


def negations(globs) -> list[str]:
    """Entries written as minimatch leading-``!`` negations."""
    return [glob for glob in globs if glob.startswith("!")]


def subtree_globs(globs, root: str) -> list[str]:
    """Sorted wildcard entries that reach into ``root``, negations excluded."""
    return sorted(
        glob
        for glob in globs
        if not glob.startswith("!") and is_wildcard(glob) and subtree_root(glob) == root
    )


def subtree_roots(globs) -> list[str]:
    """Sorted package subtrees the non-negated wildcard entries reach into."""
    roots = {
        root
        for glob in globs
        if not glob.startswith("!")
        and is_wildcard(glob)
        and (root := subtree_root(glob)) is not None
    }
    return sorted(roots)


def glob_problems(packages) -> list[str]:
    """One message per `[[package]]` glob entry that would republish a no-op."""
    problems = []
    for package in packages:
        name = package.get("name", "<unnamed>")
        globs = package.get("globs", [])
        for glob in negations(globs):
            problems.append(
                f'{name}: glob "{glob}" is a leading-`!` negation, which '
                f"putitoutthere does not support -- its matcher ORs the globs "
                f"together, so the negation subtracts nothing and instead "
                f"matches every path outside its own subtree. Write the "
                f"exclusion as an extglob inside a positive pattern: "
                f'"{carve_out("<root>")[1]}".'
            )
        for root in subtree_roots(globs):
            found = subtree_globs(globs, root)
            expected = carve_out(root)
            if found != sorted(expected):
                problems.append(
                    f"{name}: publish globs for {root} are "
                    f"{', '.join(found)} -- expected {', '.join(expected)}. "
                    f"A subtree glob that does not carve out "
                    f"{', '.join((*NON_SHIPPING_DIRS, *NON_SHIPPING_FILES))} "
                    f"republishes the package for changes that never reach its "
                    f"artifact. Replace the entries above in putitoutthere.toml."
                )
    return problems


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
