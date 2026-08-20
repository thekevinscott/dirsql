"""Derive the testing-conventions gate matrix from the workflows that call it (#781).

The justfile's `test-conventions` recipe restated the (source, gates) pairs by
hand and covered 3 of the 8 scan roots CI declares, so a locally-green run said
nothing about most of the pairs. Reading the workflows makes that drift
impossible: a caller added to `.github/workflows/` is a pair `preflight` runs.

Which workflows hold those callers is itself discovered rather than named. The
matrix used to come from `conventions.yml` alone; #834 split it into six
per-domain workflows and deleted it, so the named default resolved to nothing
and `just preflight` died on a bare `FileNotFoundError` (#973). A list of six
names would only move the same failure one rename away.
"""

from __future__ import annotations

import json
import os
from collections.abc import Callable, Sequence
from dataclasses import dataclass

import yaml

REUSABLE = ".github/workflows/testing-conventions.yml"
WORKFLOWS = ".github/workflows"

# The reusable workflow's `gates` input defaults to '' -- "every applicable
# gate". Names are the allowlist from its own input description; a root that
# omits `gates:` gets all of them.
ROOT_GATES = [
    "colocated-test",
    "unit-lint",
    "integration-lint",
    "unit-coverage",
    "mutation",
    "packaging",
    "e2e-verify",
]


class NoGateMatrix(Exception):
    """No workflow to derive the matrix from -- carries the contributor-facing fix."""


@dataclass
class Gate:
    """A gate's CLI subcommand and which of the shared options it accepts.

    The mapping is neither mechanical nor uniform: `mutation` is `unit
    mutation`, `packaging` takes no `--config`, `e2e verify` takes neither
    `--language` nor `--config`, and only the diff-scoped gates take `--base`.
    Passing an option a subcommand does not declare is a hard argv error, so
    the table is the contract.
    """

    command: tuple[str, ...]
    language: bool = True
    config: bool = True
    base: bool = False
    # Executes the package's test suite, so it needs that package's own venv on
    # `python3`, not the throwaway one `uvx` supplies.
    runs_suite: bool = False
    # Takes the root of a BUILT artifact, not a source dir. CI builds it from
    # the manifest first; preflight has no equivalent and reports these skipped.
    needs_artifact: bool = False


GATES = {
    "colocated-test": Gate(("unit", "colocated-test"), base=True),
    "unit-lint": Gate(("unit", "lint")),
    "integration-lint": Gate(("integration", "lint")),
    "unit-coverage": Gate(("unit", "coverage"), base=True, runs_suite=True),
    "mutation": Gate(("unit", "mutation"), base=True, runs_suite=True),
    "packaging": Gate(("packaging",), config=False, needs_artifact=True),
    "e2e-verify": Gate(("e2e", "verify"), language=False, config=False, base=True),
}


@dataclass
class Root:
    job: str
    source: str
    languages: list[str]
    gates: list[str]
    # "" rather than None when a caller declares no config, keeping every path
    # in the matrix a plain str.
    config: str = ""


def parse_gate_matrix(text: str) -> list[Root]:
    """Every job in `text` that calls the reusable workflow, as a Root.

    Tolerant of a document that declares no jobs: discovery hands this every
    file in `.github/workflows`, and most of them call nothing.
    """
    document = yaml.safe_load(text)
    if not isinstance(document, dict):
        return []
    roots = []
    for job, spec in (document.get("jobs") or {}).items():
        if REUSABLE not in spec.get("uses", ""):
            continue
        inputs = spec.get("with", {})
        roots.append(
            Root(
                job=job,
                source=inputs["source"],
                languages=json.loads(inputs.get("languages") or "[]"),
                gates=json.loads(inputs["gates"]) if inputs.get("gates") else list(ROOT_GATES),
                config=inputs.get("config", ""),
            )
        )
    return roots


def read_text(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def named(path: str, read: Callable[[str], str]) -> str:
    """The text of an explicitly named workflow, or a NoGateMatrix carrying the fix."""
    try:
        text = read(path)
    except OSError as err:
        raise NoGateMatrix(
            f"--conventions {path}: no such workflow. Name one that calls {REUSABLE}, "
            f"or drop the flag to derive the matrix from every caller in {WORKFLOWS}."
        ) from err
    if not parse_gate_matrix(text):
        raise NoGateMatrix(
            f"--conventions {path}: no job in it calls {REUSABLE}, so it declares no "
            f"gates. Name a workflow that does, or drop the flag to derive the matrix "
            f"from every caller in {WORKFLOWS}."
        )
    return text


def discovered(
    directory: str,
    listdir: Callable[[str], list[str]],
    read: Callable[[str], str],
) -> list[tuple[str, str]]:
    """(path, text) for every workflow in `directory` holding a caller."""
    try:
        names = sorted(listdir(directory))
    except OSError as err:
        raise NoGateMatrix(
            f"no {directory} directory here. Run preflight from the repo root, or "
            "name a workflow with --conventions."
        ) from err
    found = []
    for name in names:
        if not name.endswith((".yml", ".yaml")):
            continue
        path = f"{directory}/{name}"
        text = read(path)
        if parse_gate_matrix(text):
            found.append((path, text))
    if not found:
        raise NoGateMatrix(
            f"no workflow in {directory} calls {REUSABLE}, so there is no gate matrix "
            "to run. If the reusable workflow moved, update REUSABLE in "
            "internals/checks/src/checks/preflight/matrix.py; otherwise name the "
            "workflow with --conventions."
        )
    return found


def sources(
    conventions: Sequence[str],
    *,
    directory: str = WORKFLOWS,
    listdir: Callable[[str], list[str]] = os.listdir,
    read: Callable[[str], str] = read_text,
) -> list[tuple[str, str]]:
    """(path, text) for the workflows the gate matrix is derived from."""
    if conventions:
        return [(path, named(path, read)) for path in conventions]
    return discovered(directory, listdir, read)


def pairs(roots: list[Root]) -> list[tuple[Root, str, str]]:
    """Flatten the matrix into the (root, language, gate) triples to run."""
    return [(root, lang, gate) for root in roots for lang in root.languages for gate in root.gates]
