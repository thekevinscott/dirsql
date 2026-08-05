"""Derive the testing-conventions gate matrix from `.github/workflows/conventions.yml` (#781).

The justfile's `test-conventions` recipe restated the (source, gates) pairs by
hand and covered 3 of the 8 scan roots CI declares, so a locally-green run said
nothing about 34 of the ~40 pairs. Reading the workflow makes that drift
impossible: a caller added to `conventions.yml` is a pair `preflight` runs.
"""

from __future__ import annotations

import json
from dataclasses import dataclass

import yaml

REUSABLE = ".github/workflows/testing-conventions.yml"

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
    """Every `conventions.yml` job that calls the reusable workflow, as a Root."""
    roots = []
    for job, spec in yaml.safe_load(text)["jobs"].items():
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


def pairs(roots: list[Root]) -> list[tuple[Root, str, str]]:
    """Flatten the matrix into the (root, language, gate) triples to run."""
    return [(root, lang, gate) for root in roots for lang in root.languages for gate in root.gates]
