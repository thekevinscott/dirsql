"""E2E test for `dirsql-checks pytest-gate` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script as
a subprocess against a real target directory and a real pytest run.
"""

from __future__ import annotations

import shutil
import subprocess

import pytest


def _cli() -> str:
    dirsql_checks = shutil.which("dirsql-checks")
    assert dirsql_checks is not None, (
        "`dirsql-checks` console script not on PATH -- run "
        "`uv run --project internals/checks pytest tests/e2e` "
        "or `uv sync --project internals/checks`"
    )
    return dirsql_checks


def describe_dirsql_checks_pytest_gate():
    def it_exits_zero_for_a_passing_directory(tmp_path):
        (tmp_path / "sample_test.py").write_text("def test_ok():\n    assert True\n")

        proc = subprocess.run(
            [_cli(), "pytest-gate", str(tmp_path), "-q"],
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"

    def it_exits_nonzero_for_a_failing_directory(tmp_path):
        (tmp_path / "sample_test.py").write_text(
            "def test_broken():\n    assert False\n"
        )

        proc = subprocess.run(
            [_cli(), "pytest-gate", str(tmp_path), "-q"],
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert proc.returncode != 0

    def it_exits_zero_for_a_directory_with_no_test_files(tmp_path):
        (tmp_path / "helper.py").write_text("x = 1\n")

        proc = subprocess.run(
            [_cli(), "pytest-gate", str(tmp_path), "-q"],
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "nothing to test" in proc.stdout
