"""Build the argv and cwd for one (root, language, gate) pair (#781).

Three pairs cannot be run the naive way, and each is encoded rather than skipped:

  * python `mutation` / `unit-coverage` -- these execute the suite, whose
    `python3 -m pytest` must resolve to the package's own venv rather than an
    ambient interpreter with no pytest (#706 saw it as a cosmic-ray baseline
    failure naming no cause). Both run under `uv run` from the package root.
  * rust `colocated-test` -- the co-change variant rides only the
    python/typescript language set (rust units are inline, so no sibling can go
    stale), and the CLI rejects `--base` for it.
  * `e2e-verify` -- its PATH is the package root holding `e2e-attestations/`,
    with the source dir as `--scope`; the `[e2e]` config table it needs is
    passed as `--extra-scope` / `--exclude` flags, since it takes no `--config`.
"""

from __future__ import annotations

import os.path
from collections.abc import Callable
from dataclasses import dataclass

from .e2e_flags import e2e_flags
from .matrix import GATES, Root
from .package_root import package_root

# The CLI, resolved the way CI resolves it. Two silent drifts made the local run
# enforce a different ruleset, and naming the tag closes both: the PyPI wheel
# `uvx` fetches lags npm by several releases, and a BARE `npx testing-conventions`
# resolves engine-aware, so a shim on node older than `engines.node` picks the
# last release below that floor -- and exits 0 with no banner to say so.
CLI = ["npx", "-y", "testing-conventions@latest"]

# How to launch a suite-executing gate, per language. Both run from the package
# root: python so the suite's `python3 -m pytest` resolves to that package's
# venv, typescript because only the npm CLI appends `--ts-mutation-adapter`,
# which the rule needs. Rust is absent -- it has no suite environment to enter.
#
# The python arm needs BOTH distributions. `--with` is not how the CLI is
# resolved; it puts the wheel's `testing_conventions` package on the interpreter
# the gate hands to `python3 -m`, which is the only form the mutation adapter
# ships in.
LAUNCHERS = {
    "python": ["uv", "run", "--with", "testing-conventions", *CLI],
    "typescript": CLI,
}


@dataclass
class Invocation:
    argv: list[str]
    # "." rather than None for the repo root: `subprocess.run(cwd=".")` is the
    # same call, and it keeps every path in this module a plain str.
    cwd: str


def invocation(
    root: Root,
    language: str,
    gate_name: str,
    base: str,
    exists: Callable[[str], bool],
    e2e: dict,
) -> Invocation:
    gate = GATES[gate_name]
    options = []
    if gate.language:
        options += ["--language", language]
    if gate.base and (gate_name, language) != ("colocated-test", "rust"):
        options += ["--base", base]
    if gate.config and root.config:
        options += ["--config", root.config]

    if gate_name == "e2e-verify":
        home = package_root(root.source, exists)
        return Invocation(
            [
                *[*CLI, *gate.command, *options],
                *["--scope", root.source, *e2e_flags(e2e), home],
            ],
            ".",
        )
    if gate.runs_suite and language in LAUNCHERS:
        # Paths become relative to the package root we run from, not the repo.
        cwd = package_root(root.source, exists)
        if gate.config and root.config:
            options[-1] = os.path.relpath(root.config, cwd)
        source = os.path.relpath(root.source, cwd)
        return Invocation([*LAUNCHERS[language], *gate.command, *options, source], cwd)
    return Invocation([*CLI, *gate.command, *options, root.source], ".")
