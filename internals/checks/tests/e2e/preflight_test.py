"""E2E test for `dirsql-checks preflight` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script from
the repo root, where its defaults have to resolve the real `.github/workflows/`
on their own. `--dry-run` prints the derived matrix without running a gate.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]


def _cli() -> str:
    dirsql_checks = shutil.which("dirsql-checks")
    assert dirsql_checks is not None, (
        "`dirsql-checks` console script not on PATH -- run "
        "`uv run --project internals/checks pytest tests/e2e` "
        "or `uv sync --project internals/checks`"
    )
    return dirsql_checks


def preflight(*args) -> subprocess.CompletedProcess:
    return subprocess.run(
        [_cli(), "preflight", "--dry-run", *args],
        cwd=REPO,
        capture_output=True,
        text=True,
        timeout=120,
    )


def describe_dirsql_checks_preflight():
    def it_prints_the_matrix_from_the_repo_root_with_no_arguments():
        proc = preflight()

        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "==> python-sdk [python] unit-lint" in proc.stdout
        assert "==> typescript-sdk [typescript] unit-lint" in proc.stdout
        assert "==> rust [rust] unit-lint" in proc.stdout
        assert "==> internals-checks [python] unit-lint" in proc.stdout

    def it_exits_with_an_actionable_message_when_a_named_workflow_is_missing():
        proc = preflight("--conventions", ".github/workflows/conventions.yml")

        assert proc.returncode == 1
        assert "Traceback" not in proc.stderr
        assert ".github/workflows/conventions.yml" in proc.stdout + proc.stderr
