"""E2E test for `dirsql-checks changelog-gate` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script as
a subprocess, run against a real scratch git repo.
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


def _git(repo, *args: str) -> None:
    subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, text=True)


def _commit(repo, message: str) -> str:
    _git(repo, "add", "-A")
    _git(repo, "commit", "-m", message)
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=repo, check=True, capture_output=True, text=True
    )
    return result.stdout.strip()


@pytest.fixture
def repo(tmp_path):
    _git(tmp_path, "init", "-q")
    _git(tmp_path, "config", "user.email", "test@example.com")
    _git(tmp_path, "config", "user.name", "Test")
    (tmp_path / "README.md").write_text("hello\n")
    base_sha = _commit(tmp_path, "initial commit")
    return tmp_path, base_sha


def describe_dirsql_checks_changelog_gate():
    def it_exits_nonzero_when_sdk_code_changes_without_a_changelog_entry(repo):
        tmp_path, base_sha = repo
        rust_dir = tmp_path / "packages" / "rust" / "src"
        rust_dir.mkdir(parents=True)
        (rust_dir / "lib.rs").write_text("// code\n")
        head_sha = _commit(tmp_path, "add sdk code")

        proc = subprocess.run(
            [
                _cli(),
                "changelog-gate",
                "--base-sha",
                base_sha,
                "--head-sha",
                head_sha,
            ],
            cwd=tmp_path,
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert proc.returncode == 1
        assert "SDK code changed" in proc.stderr

    def it_exits_zero_when_the_changelog_gains_an_entry(repo):
        tmp_path, base_sha = repo
        rust_dir = tmp_path / "packages" / "rust" / "src"
        rust_dir.mkdir(parents=True)
        (rust_dir / "lib.rs").write_text("// code\n")
        (tmp_path / "CHANGELOG.md").write_text("## [Unreleased]\n- Added a thing\n")
        head_sha = _commit(tmp_path, "add sdk code with changelog")

        proc = subprocess.run(
            [
                _cli(),
                "changelog-gate",
                "--base-sha",
                base_sha,
                "--head-sha",
                head_sha,
            ],
            cwd=tmp_path,
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
