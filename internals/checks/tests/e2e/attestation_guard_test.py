"""E2E test for `dirsql-checks attestation-guard` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script as
a subprocess, run against a real scratch git repo.
"""

from __future__ import annotations

import shutil
import subprocess

import pytest

RECEIPT = '{"command": "pytest", "exit_code": 0}\n'


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


def _head(repo) -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=repo, check=True, capture_output=True, text=True
    )
    return result.stdout.strip()


def _commit(repo, message: str) -> str:
    _git(repo, "add", "-A")
    _git(repo, "commit", "-m", message)
    return _head(repo)


def _write(path, text=RECEIPT):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def _guard(repo, base_sha, head_sha):
    return subprocess.run(
        [_cli(), "attestation-guard", "--base-sha", base_sha, "--head-sha", head_sha],
        cwd=repo,
        capture_output=True,
        text=True,
        timeout=30,
    )


@pytest.fixture
def repo(tmp_path):
    _git(tmp_path, "init", "-q")
    _git(tmp_path, "config", "user.email", "test@example.com")
    _git(tmp_path, "config", "user.name", "Test")
    (tmp_path / "README.md").write_text("hello\n")
    _write(tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json")
    base_sha = _commit(tmp_path, "initial commit")
    return tmp_path, base_sha


def describe_dirsql_checks_attestation_guard():
    def it_exits_zero_when_the_branch_only_adds_its_own_receipt(repo):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "ts" / "e2e-attestations" / "mine.json")
        head_sha = _commit(tmp_path, "attest my branch")

        proc = _guard(tmp_path, base_sha, head_sha)
        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"

    def it_exits_nonzero_when_the_branch_deletes_a_foreign_receipt(repo):
        tmp_path, base_sha = repo
        (tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json").unlink()
        head_sha = _commit(tmp_path, "attest (pruned a sibling)")

        proc = _guard(tmp_path, base_sha, head_sha)
        assert proc.returncode == 1, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "packages/ts/e2e-attestations/old-branch.json" in proc.stdout
        assert f"git checkout {base_sha} --" in proc.stdout

    def it_exits_zero_via_the_allow_receipt_deletion_line(repo):
        tmp_path, base_sha = repo
        (tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json").unlink()
        head_sha = _commit(tmp_path, "retire ts\n\nallow-receipt-deletion: package removed")

        proc = _guard(tmp_path, base_sha, head_sha)
        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "permitting receipt deletion" in proc.stdout

    def it_names_a_near_miss_bypass(repo):
        tmp_path, base_sha = repo
        (tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json").unlink()
        head_sha = _commit(tmp_path, "retire ts\n\nskip-receipt-deletion: package removed")

        proc = _guard(tmp_path, base_sha, head_sha)
        assert proc.returncode == 1, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "is not the bypass line" in proc.stdout
