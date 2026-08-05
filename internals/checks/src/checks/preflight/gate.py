"""Run every (root, language, gate) pair CI declares, and report real exit codes (#781).

Two failure modes this exists to remove: a hand-maintained pair list that drifts
from `conventions.yml`, and hand-transcribed shell one-liners whose non-zero exit
gets masked (`... ; echo ok`). Here the argv is derived and the exit code is the
return value.

Before the gates, each python root gets two drift guards (#782): `uv sync`, which
reconciles the venv with the manifest, and `declared-deps`, which asserts every
import is declared. A `uv pip install` leaves the local venv strictly more capable
than any real install, and no amount of running more gates catches that.

Three pairs cannot be run the naive way, and each is encoded rather than skipped:

  * python `mutation` / `unit-coverage` -- `uvx` supplies its own environment, so
    the suite's `python3 -m pytest` resolves to an interpreter with no pytest
    (#706 saw it as a cosmic-ray baseline failure naming no cause). Both run via
    the package's own venv instead.
  * rust `colocated-test` -- the co-change variant rides only the
    python/typescript language set (rust units are inline, so no sibling can go
    stale), and the CLI rejects `--base` for it.
  * `e2e-verify` -- its PATH is the package root holding `e2e-attestations/`,
    with the source dir as `--scope`; the `[e2e]` config table it needs is
    passed as `--extra-scope` / `--exclude` flags, since it takes no `--config`.
"""

from __future__ import annotations

import os.path
import subprocess
import tomllib
from collections.abc import Callable, Sequence
from dataclasses import dataclass

from .matrix import GATES, Root, pairs, parse_gate_matrix

MANIFESTS = ("pyproject.toml", "package.json", "Cargo.toml")

# How to launch a suite-executing gate, per language. Both run from the package
# root: python so `python3 -m pytest` resolves to that package's venv, typescript
# because only the npm CLI appends `--ts-mutation-adapter`, which the rule needs.
# Rust is absent -- it has no suite environment to enter, so it stays on `uvx`.
LAUNCHERS = {
    "python": ["uv", "run", "--with", "testing-conventions", "testing-conventions"],
    "typescript": ["npx", "-y", "testing-conventions"],
}


@dataclass
class Invocation:
    argv: list[str]
    # "." rather than None for the repo root: `subprocess.run(cwd=".")` is the
    # same call, and it keeps every path in this module a plain str.
    cwd: str


def package_root(source: str, exists: Callable[[str], bool]) -> str:
    """Nearest ancestor of `source` (inclusive) holding a package manifest."""
    parts = source.split("/")
    while parts:
        candidate = "/".join(parts)
        if any(exists(f"{candidate}/{name}") for name in MANIFESTS):
            return candidate
        parts.pop()
    return "."


def e2e_flags(e2e: dict) -> list[str]:
    flags = []
    for scope in e2e.get("extra_scope", []):
        flags += ["--extra-scope", scope]
    for path in e2e.get("exclude", []):
        flags += ["--exclude", path]
    return flags


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
                *["uvx", "testing-conventions", *gate.command, *options],
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
    return Invocation(["uvx", "testing-conventions", *gate.command, *options, root.source], ".")


def default_runner(argv: Sequence[str], cwd: str) -> int:
    return subprocess.run(argv, cwd=cwd, check=False).returncode


def read_e2e(config: str) -> dict:
    """The `[e2e]` table of a root's testing-conventions config, if it has one."""
    if not config or not os.path.exists(config):
        return {}
    with open(config, "rb") as handle:
        return tomllib.load(handle).get("e2e", {})


def prepare(roots: list[Root], exists: Callable[[str], bool]) -> list[tuple[str, str, Invocation]]:
    """Per-python-root steps that guard against venv drift (#782).

    `uv sync` reconciles the venv with the manifest, *removing* anything a
    `uv pip install` left behind, so an undeclared dependency stops resolving
    locally the way it never resolved in CI. `declared-deps` is the direct
    assertion, independent of whatever the venv happens to hold.
    """
    steps = []
    for root in roots:
        if "python" not in root.languages:
            continue
        home = package_root(root.source, exists)
        steps.append((root.job, "uv-sync", Invocation(["uv", "sync", "--project", home], ".")))
        steps.append(
            (
                root.job,
                "declared-deps",
                Invocation(
                    [
                        *["uv", "run", "--project", "internals/checks", "dirsql-checks"],
                        *["declared-deps", root.source],
                    ],
                    ".",
                ),
            )
        )
    return steps


def run(
    conventions: str,
    base: str,
    *,
    runner: Callable[[Sequence[str], str], int],
    exists: Callable[[str], bool],
    e2e_config: Callable[[str], dict],
    echo: Callable[[str], None],
    only: Sequence[str] = (),
    dry_run: bool = False,
) -> int:
    """Run the whole matrix; return 0 only when every pair passed."""
    roots = parse_gate_matrix(conventions)
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
