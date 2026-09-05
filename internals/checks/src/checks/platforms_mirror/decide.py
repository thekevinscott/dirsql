"""Pure decision logic for the platforms-mirror check (#1004).

`internals/distcheck`'s `Platform` dataclass is a hand-maintained subset of
`packages/ts/src/platforms.ts`, which is the release source of truth for the
published `@dirsql/lib-*` sub-packages. The subset framing is deliberate --
distcheck needs only enough of each target to reconstruct the host's
sub-package and find its staged addon -- but it was asserted by a comment and
checked by nothing, and it drifted both ways: `exe` sat on the Python side
justified as mirroring TypeScript, which has never had the field.

So the invariant here is the subset, stated as data: `SHARED` names every
Python field and where its value comes from, both tables carry the same set of
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


def _slug(lib_name: str) -> str:
    return lib_name.removeprefix(LIB_PREFIX)


# Python field -> how its TypeScript value is derived, for the fields that are
# not a straight read. Keyed rather than branched so the field name is never
# compared, only looked up.
DERIVE = {"slug": _slug}


def key(node_platform: str, node_arch: str) -> str:
    return f"{node_platform}-{node_arch}"


def unmirrored_fields(fields) -> list[str]:
    """One message per dataclass field with no TypeScript source."""
    return [
        f"Platform.{field} has no counterpart in {TYPESCRIPT_FILE}. platforms.py "
        f"holds a deliberate subset of the published target ({', '.join(SHARED)}), "
        f"so a field the mirror cannot source is drift: delete it, or add the "
        f"property to the TypeScript `Platform` interface and to SHARED here."
        for field in fields
        if field not in SHARED
    ]


def typescript_value(field: str, row: dict):
    """``row``'s value for a Python field name."""
    value = row.get(SHARED[field])
    derive = DERIVE.get(field)
    if derive is None or not isinstance(value, str):
        return value
    return derive(value)


def prefix_problems(rows) -> list[str]:
    """One message per `libName` that `librarySlug()` would throw on."""
    return [
        f"{key(row.get('nodePlatform'), row.get('nodeArch'))}: libName "
        f"{row.get('libName')!r} does not start with {LIB_PREFIX!r}, so "
        f"`librarySlug()` throws on it and the Python `slug` cannot be derived. "
        f"Fix the name in {TYPESCRIPT_FILE}."
        for row in rows
        if not str(row.get("libName", "")).startswith(LIB_PREFIX)
    ]


def missing_rows(python_keys, typescript_rows) -> list[str]:
    """One message per published target with no row in platforms.py."""
    return [
        f"{key(row['nodePlatform'], row['nodeArch'])} is published by platforms.ts "
        f"but has no row in platforms.py. Add "
        f"Platform({row['nodePlatform']!r}, {row['nodeArch']!r}, "
        f"{_slug(str(row.get('libName', '')))!r}, {row.get('os')!r}, "
        f"{row.get('cpu')!r}) to PLATFORMS in {PYTHON_FILE}, or the node distcheck "
        f"flow cannot resolve the new target on that host."
        for row in typescript_rows
        if key(row["nodePlatform"], row["nodeArch"]) not in python_keys
    ]


def stray_rows(typescript_keys, python_rows) -> list[str]:
    """One message per platforms.py row that is not a published target."""
    return [
        f"{key(row['node_platform'], row['node_arch'])} has a row in platforms.py "
        f"but is not a published target in platforms.ts. Either add the target to "
        f"PLATFORMS in {TYPESCRIPT_FILE} or drop the row from {PYTHON_FILE}: "
        f"distcheck must not resolve a sub-package the release never publishes."
        for row in python_rows
        if key(row["node_platform"], row["node_arch"]) not in typescript_keys
    ]

