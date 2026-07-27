"""Correct-invocation Rust mutation gate (#672).

testing-conventions' built-in `unit mutation --language rust` feeds cargo-mutants a
`git diff --relative` (crate-relative paths). But `packages/rust` is a cargo *workspace*
member, so cargo-mutants matches `--in-diff` hunks by *workspace-relative* path: the two
never line up, every mutant is filtered out, and the job reports `0 mutant(s) tested` and
exits 0 -- a false green that gave Rust mutation testing zero protection in CI.

This gate drives cargo-mutants directly with a workspace-relative diff (no `--relative`),
so a PR's changed Rust lines are actually mutated. A PR that touches no mutatable Rust line
yields an empty match set and passes trivially, exactly as the diff-scoped gate should.
"""
from __future__ import annotations

import subprocess
import tempfile

CRATE_DIR = "packages/rust"

SURVIVOR_HINT = (
    "cargo-mutants reported a problem (a surviving mutant, timeout, or build failure). "
    "A surviving mutant means a PR-changed line has no unit test that fails when its "
    "behavior is altered -- add or strengthen an assertion in that file's #[cfg(test)] "
    "module to kill it; never weaken a test."
)


def _write_temp_diff(diff):
    handle = tempfile.NamedTemporaryFile("w", suffix=".diff", delete=False)
    handle.write(diff)
    handle.close()
    return handle.name


def build_diff(base, runner):
    # No `--relative`: cargo-mutants matches --in-diff hunks by workspace-relative
    # path (packages/rust is a cargo workspace member), so the diff must carry
    # workspace-root paths. `--relative` yields crate-relative paths that match nothing.
    result = runner(
        ["git", "diff", f"{base}...HEAD"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout


def run(base, runner=subprocess.run, writer=_write_temp_diff):
    diff = build_diff(base, runner)
    diff_path = writer(diff)
    result = runner(
        ["cargo", "mutants", "--features", "cli", "--in-diff", diff_path],
        cwd=CRATE_DIR,
    )
    if result.returncode:
        print(SURVIVOR_HINT)
    return result.returncode
