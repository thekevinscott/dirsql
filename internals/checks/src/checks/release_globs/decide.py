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


def is_wildcard(glob: str) -> bool:
    """True when the entry is a pattern rather than a literal path."""
    return any(character in glob for character in _GLOB_META)


def negations(globs) -> list[str]:
    """Entries written as minimatch leading-``!`` negations."""
    return [glob for glob in globs if glob.startswith("!")]
