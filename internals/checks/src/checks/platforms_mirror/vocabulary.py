"""What the two platform tables share, and where each Python field comes from.

`internals/distcheck`'s `Platform` dataclass is a hand-maintained subset of
`packages/ts/src/platforms.ts`, which is the release source of truth for the
published `@dirsql/lib-*` sub-packages. The subset framing is deliberate --
distcheck needs only enough of each target to reconstruct the host's
sub-package and find its staged addon -- but it was asserted by a comment and
checked by nothing, and it drifted both ways: `exe` sat on the Python side
justified as mirroring TypeScript, which has never had the field.

So the invariant is the subset, stated as data: `SHARED` names every Python
field and where its value comes from, both tables carry the same set of
targets, and a Python field outside `SHARED` is drift by construction.
`triple` and `libc` staying TypeScript-only is the subset working as intended,
not a gap.
"""

from __future__ import annotations

PYTHON_FILE = "internals/distcheck/src/distcheck/node_flow/platforms.py"
TYPESCRIPT_FILE = "packages/ts/src/platforms.ts"

# Python field -> the TypeScript property it is sourced from. `slug` is the
# sub-package name minus its prefix, which is what `librarySlug()` returns.
SHARED = {
    "node_platform": "nodePlatform",
    "node_arch": "nodeArch",
    "slug": "libName",
    "os": "os",
    "cpu": "cpu",
}
LIB_PREFIX = "@dirsql/lib-"


def slug(lib_name: str) -> str:
    return lib_name.removeprefix(LIB_PREFIX)


# Python field -> how its TypeScript value is derived, for the fields that are
# not a straight read. Keyed rather than branched so the field name is never
# compared, only looked up.
DERIVE = {"slug": slug}


def key(node_platform: str, node_arch: str) -> str:
    return f"{node_platform}-{node_arch}"
