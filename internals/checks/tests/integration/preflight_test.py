"""Integration tests for `dirsql-checks preflight` against the repo's real workflows.

The colocated unit tests feed `parse_gate_matrix` inline YAML fixtures, so nothing
noticed when #834 split `conventions.yml` into six per-domain workflows and the
command's default target stopped existing (#973). These invoke the command with
its real defaults over the real `.github/workflows/`, the only place that drift
shows: a caller the matrix misses is a lane `just preflight` silently skips.
"""

from __future__ import annotations

import os
from pathlib import Path

import pytest
import yaml
from click.testing import CliRunner

from checks.preflight.cli import cli
from checks.preflight.matrix import REUSABLE

REPO = Path(__file__).resolve().parents[4]
WORKFLOWS = REPO / ".github" / "workflows"


def caller_jobs() -> dict[str, str]:
    """Every reusable-workflow caller in the real workflows, as job -> workflow file.

    Walked from parsed YAML rather than from preflight's own discovery, so the two
    agree only when discovery is right.
    """
    jobs = {}
    for path in sorted(WORKFLOWS.glob("*.yml")):
        document = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
        for job, spec in (document.get("jobs") or {}).items():
            if REUSABLE in (spec.get("uses") or ""):
                jobs[job] = path.name
    return jobs


@pytest.fixture
def repo_root():
    """The command resolves `.github/workflows` relative to the cwd, as a user does."""
    previous = os.getcwd()
    os.chdir(REPO)
    yield REPO
    os.chdir(previous)


def dry_run(*args):
    return CliRunner().invoke(cli, ["--dry-run", *args])


def describe_preflight_over_the_real_workflows():
    def it_derives_a_matrix_with_no_arguments(repo_root):
        result = dry_run()

        assert result.exit_code == 0, f"{result.output}{result.exception!r}"
        assert result.output.count("==> ") > 1

    def it_covers_every_reusable_workflow_caller_the_repo_declares(repo_root):
        jobs = caller_jobs()
        assert jobs, f"no caller job found under {WORKFLOWS}"

        output = dry_run().output

        assert {job for job in jobs if f"==> {job} [" in output} == set(jobs)

    def it_spans_every_workflow_holding_a_caller_not_just_one(repo_root):
        # The #834 shape: the callers live in six files, so a matrix read from a
        # single workflow is a green run that covers one lane.
        jobs = caller_jobs()
        assert len(set(jobs.values())) > 1

        output = dry_run().output

        assert {file for job, file in jobs.items() if f"==> {job} [" in output} == set(jobs.values())

    def it_derives_the_one_function_per_file_gate_for_a_lane_naming_no_gates(repo_root):
        # `@v0` runs `unit one-function-per-file` in the static job, so a lane
        # that names no `gates:` is held to it in CI. A matrix omitting it is a
        # locally-green run that says nothing about the gate.
        output = dry_run().output

        assert "==> python-sdk [python] one-function-per-file" in output
        assert "==> typescript-sdk [typescript] one-function-per-file" in output

    def it_derives_the_one_function_per_file_gate_for_every_lane_naming_it(repo_root):
        # An explicit `gates` array is an allowlist: a lane that omits the gate
        # does not run it, however the reusable workflow defaults.
        output = dry_run().output

        assert "==> internals-checks [python] one-function-per-file" in output
        assert "==> internals-distcheck [python] one-function-per-file" in output
        assert "==> plugins-embeddings [python] one-function-per-file" in output

    def it_names_a_missing_workflow_and_the_fix_instead_of_raising(repo_root):
        result = dry_run("--conventions", ".github/workflows/conventions.yml")

        assert result.exit_code == 1
        assert not isinstance(result.exception, FileNotFoundError)
        message = result.output + result.stderr
        assert ".github/workflows/conventions.yml" in message
        assert "--conventions" in message
