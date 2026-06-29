#!/usr/bin/env python3
"""Decide which SDK packages a pull request changed, for the e2e-attestation
gate (``.github/workflows/e2e-attestation.yml``).

A package counts as "changed" when the ``base..head`` diff touches any file
under ``packages/<pkg>`` other than that package's own
``e2e-attestation.json`` -- so a re-attest (or the initial seed) does not
require a fresh attestation of itself, and a PR that does not touch a package
never triggers that package's ``verify``. The result is written to
``$GITHUB_OUTPUT`` as ``python=<bool>`` / ``ts=<bool>`` for the workflow's
per-package verify steps to gate on.

Lives here, with a colocated unit test, per AGENTS.md "CI Workflows":
workflow YAML stays trivial glue and any real logic moves to a tested script.
"""

from __future__ import annotations

import os
import subprocess
import sys

PACKAGES = ("python", "ts")


def package_changed(package: str, base: str, head: str) -> bool:
    """True if this PR touches a file under ``packages/<package>`` other than
    that package's ``e2e-attestation.json``.

    Uses a three-dot (``base...head``) diff, i.e. from the merge-base of
    ``base`` and ``head`` to ``head`` -- the PR's own changes. A two-dot
    ``base head`` diff would also pick up commits that landed on ``base``
    (``main``) after the branch diverged, falsely flagging packages the PR
    never touched once ``main`` advances."""
    diff = subprocess.run(
        [
            "git",
            "diff",
            "--name-only",
            f"{base}...{head}",
            "--",
            f"packages/{package}",
            f":(exclude)packages/{package}/e2e-attestation.json",
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    return bool(diff.stdout.strip())


def main(argv: list[str]) -> int:
    base, head = argv[1], argv[2]
    changed = {pkg: package_changed(pkg, base, head) for pkg in PACKAGES}

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with open(github_output, "a", encoding="utf-8") as handle:
            for pkg in PACKAGES:
                handle.write(f"{pkg}={'true' if changed[pkg] else 'false'}\n")

    for pkg in PACKAGES:
        print(f"{pkg} package changed: {'yes' if changed[pkg] else 'no'}")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main(sys.argv))
