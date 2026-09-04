"""Run every (root, language, gate) pair CI declares, and report real exit codes (#781).

Two failure modes this exists to remove: a hand-maintained pair list that drifts
from what CI declares, and hand-transcribed shell one-liners whose non-zero exit
gets masked (`... ; echo ok`). Here the argv is derived and the exit code is the
return value.

Before the gates, each python root gets two drift guards (#782): `uv sync`, which
reconciles the venv with the manifest, and `declared-deps`, which asserts every
import is declared.
"""

from __future__ import annotations

import os.path
import subprocess
import tomllib
from collections.abc import Callable, Sequence

from .invocation import Invocation, invocation
from .matrix import GATES, pairs, parse_gate_matrix
from .prepare import prepare


def default_runner(argv: Sequence[str], cwd: str) -> int:
    return subprocess.run(argv, cwd=cwd, check=False).returncode


def read_e2e(config: str) -> dict:
    """The `[e2e]` table of a root's testing-conventions config, if it has one."""
    if not config or not os.path.exists(config):
        return {}
    with open(config, "rb") as handle:
        return tomllib.load(handle).get("e2e", {})


def run(
    workflows: Sequence[str],
    base: str,
    *,
    runner: Callable[[Sequence[str], str], int],
    exists: Callable[[str], bool],
    e2e_config: Callable[[str], dict],
    echo: Callable[[str], None],
    only: Sequence[str] = (),
    dry_run: bool = False,
) -> int:
    """Run the whole matrix; return 0 only when every pair passed.

    `workflows` is the text of each workflow holding callers -- six of them post
    #834, so the roots are the concatenation rather than one file's (#973).
    """
    roots = [root for text in workflows for root in parse_gate_matrix(text)]
    failures = []
    skipped = []

    def attempt(label: str, call: Invocation) -> None:
        echo(f"==> {label}: {' '.join(call.argv)}")
        if not dry_run and runner(call.argv, call.cwd) != 0:
            failures.append(label)

    for job, step, call in prepare(roots, exists):
        if only and step not in only:
            continue
        attempt(f"{job} [python] {step}", call)
    for root, language, gate in pairs(roots):
        if only and gate not in only:
            continue
        label = f"{root.job} [{language}] {gate}"
        if GATES[gate].needs_artifact:
            skipped.append(label)
            echo(f"SKIP {label}: needs a built artifact, which CI builds from the manifest")
            continue
        attempt(label, invocation(root, language, gate, base, exists, e2e_config(root.config)))
    for label in failures:
        echo(f"FAIL {label}")
    echo(f"preflight: {len(failures)} failing pair(s), {len(skipped)} skipped")
    return 1 if failures else 0
