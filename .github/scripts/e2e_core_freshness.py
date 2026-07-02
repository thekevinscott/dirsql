#!/usr/bin/env python3
"""Gate: a non-``cli`` change to the shared Rust core must stale BOTH binding
e2e attestations (#337).

The e2e-attestation freshness gate (``.github/workflows/e2e-attestation.yml``)
runs ``testing-conventions e2e verify`` inside each binding package, and verify
walks history scoped to that binding's own git subtree
(``packages/python/**`` / ``packages/ts/**``). The shared Rust core
(``packages/rust/src/**``) is compiled into both binding artifacts but lives in
neither subtree, so a core change that alters SDK-observable behavior (row
diffing, SQLite semantics, event payloads, scan ordering, ...) propagates into
both ``.whl`` / ``.node`` at runtime yet stales neither binding attestation.
The gate would stay green without either binding's e2e suite having run against
the new behavior.

This script closes that blind spot. For the PR's ``base...head`` range it finds
the most recent commit touching *binding-linked* core source; if there is one,
each binding's committed ``e2e-attestation.json`` must name a commit that
**includes** it (the core commit is an ancestor of, or equal to, the attested
commit). A binding whose attestation predates the core change is stale and the
gate fails -- red until that binding is re-attested.

``cli``-only core changes are excluded: ``src/cli/**`` and ``src/bin/**`` are
feature-gated behind the ``cli`` Cargo feature (``packages/rust/Cargo.toml``,
``default = []``) and are never compiled into the bindings, so they cannot
change binding behavior and must not force needless re-attestation (#328 is
exactly this shape).

Lives here with a colocated unit test, per AGENTS.md "CI Workflows": workflow
YAML stays trivial glue and any real logic moves to a tested script.
"""

from __future__ import annotations

import json
import subprocess
import sys

BINDINGS = ("python", "ts")

# The binding-linked core subtree: all of the Rust core's source EXCEPT the
# ``cli``-gated modules. ``src/cli/**`` is ``#[cfg(feature = "cli")]`` and
# ``src/bin/**`` is a ``required-features = ["cli"]`` binary; neither compiles
# into the bindings, so both are excluded from the staling set.
CORE_PATHSPEC = (
    "packages/rust/src",
    ":(exclude)packages/rust/src/cli",
    ":(exclude)packages/rust/src/bin",
)


def latest_core_commit(base: str, head: str) -> str | None:
    """SHA of the most recent commit reachable from ``head`` but not ``base``
    that touches binding-linked core source, or ``None`` if the PR touches no
    such source.

    Two-dot ``base..head`` -- i.e. the PR's own commits since the merge-base --
    so commits that landed on ``base`` after the branch diverged don't
    masquerade as this PR's changes. (Note the dot convention inverts between
    ``git diff`` and ``git rev-list``: the scope script's ``git diff
    base...head`` and this ``git rev-list base..head`` both mean
    "merge-base..head"; ``git rev-list base...head`` would instead be the
    *symmetric* difference and wrongly pick up ``base``-side core commits.)"""
    result = subprocess.run(
        ["git", "rev-list", "-1", f"{base}..{head}", "--", *CORE_PATHSPEC],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip() or None


def attestation_commit(binding: str) -> str:
    """The commit SHA the binding's committed e2e attestation names."""
    path = f"packages/{binding}/e2e-attestation.json"
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)["commit"]


def includes(ancestor: str, descendant: str) -> bool:
    """True if ``ancestor`` is an ancestor of (or equal to) ``descendant``."""
    return (
        subprocess.run(
            ["git", "merge-base", "--is-ancestor", ancestor, descendant]
        ).returncode
        == 0
    )


def main(argv: list[str]) -> int:
    base, head = argv[1], argv[2]
    core = latest_core_commit(base, head)
    if core is None:
        print("No binding-linked core change in range; nothing to verify.")
        return 0

    stale = []
    for binding in BINDINGS:
        attested = attestation_commit(binding)
        if includes(core, attested):
            print(f"{binding}: attestation {attested[:12]} includes core {core[:12]}")
        else:
            stale.append(binding)
            print(
                f"{binding}: STALE -- attestation {attested[:12]} predates core "
                f"change {core[:12]}; re-attest this binding."
            )

    if stale:
        print(
            "Binding-linked core (non-cli) source changed but these bindings' "
            f"e2e attestations are stale: {', '.join(stale)}. Re-attest each "
            "(e.g. `cd packages/<pkg> && testing-conventions e2e attest '<cmd>'`).",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main(sys.argv))
