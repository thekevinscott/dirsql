"""Integration tests for the attestation-guard check against a real git repo.

Exercises `gate.run` with its default collaborators (the real `git_ops`
functions, real `git` subprocess calls) over a scratch repo fixture -- never
the packaged `dirsql-checks` CLI (that's the e2e tier).
"""

from __future__ import annotations

import os
import subprocess

import pytest

from checks.attestation_guard.gate import run

RECEIPT = '{"command": "pytest", "exit_code": 0}\n'


def _git(*args: str) -> None:
    subprocess.run(["git", *args], check=True, capture_output=True, text=True)


def _head() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True
    )
    return result.stdout.strip()


def _commit(message: str) -> str:
    _git("add", "-A")
    _git("commit", "-m", message)
    return _head()


def _write(path, text=RECEIPT):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


@pytest.fixture
def repo(tmp_path):
    original_cwd = os.getcwd()
    os.chdir(tmp_path)
    try:
        _git("init", "-q")
        _git("config", "user.email", "test@example.com")
        _git("config", "user.name", "Test")
        (tmp_path / "README.md").write_text("hello\n")
        _write(tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json")
        _write(tmp_path / "packages" / "python" / "e2e-attestations" / "other.json")
        base_sha = _commit("initial commit")
        yield tmp_path, base_sha
    finally:
        os.chdir(original_cwd)


def describe_run_against_a_real_repo():
    def it_passes_when_the_branch_only_adds_its_own_receipt(repo, capsys):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "ts" / "e2e-attestations" / "mine.json")
        head_sha = _commit("attest my branch")

        assert run(base_sha, head_sha) == 0
        assert "No e2e attestation receipts deleted." in capsys.readouterr().out

    def it_passes_when_the_branch_updates_an_existing_receipt(repo, capsys):
        tmp_path, base_sha = repo
        _write(
            tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json",
            '{"command": "pytest", "exit_code": 0, "commit": "x"}\n',
        )
        head_sha = _commit("re-attest")

        assert run(base_sha, head_sha) == 0

    def it_fails_when_the_branch_deletes_another_branchs_receipt(repo, capsys):
        tmp_path, base_sha = repo
        (tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json").unlink()
        head_sha = _commit("attest (pruned a sibling)")

        assert run(base_sha, head_sha) == 1
        out = capsys.readouterr().out
        assert "packages/ts/e2e-attestations/old-branch.json" in out
        assert f"git checkout {base_sha} --" in out

    def it_reports_every_deleted_receipt_across_packages(repo, capsys):
        tmp_path, base_sha = repo
        (tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json").unlink()
        (tmp_path / "packages" / "python" / "e2e-attestations" / "other.json").unlink()
        head_sha = _commit("prune everything")

        assert run(base_sha, head_sha) == 1
        assert capsys.readouterr().out.count("::error file=") == 2

    def it_fails_when_a_receipt_is_renamed_away(repo, capsys):
        tmp_path, base_sha = repo
        attestations = tmp_path / "packages" / "ts" / "e2e-attestations"
        (attestations / "old-branch.json").rename(attestations / "renamed.json")
        head_sha = _commit("rename a receipt")

        assert run(base_sha, head_sha) == 1
        assert "old-branch.json" in capsys.readouterr().out

    def it_passes_when_only_source_is_deleted(repo, capsys):
        tmp_path, base_sha = repo
        (tmp_path / "README.md").unlink()
        head_sha = _commit("drop the readme")

        assert run(base_sha, head_sha) == 0

    def it_passes_via_the_allow_receipt_deletion_line(repo, capsys):
        tmp_path, base_sha = repo
        (tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json").unlink()
        _git("add", "-A")
        _git("commit", "-m", "retire ts\n\nallow-receipt-deletion: package removed")
        head_sha = _head()

        assert run(base_sha, head_sha) == 0
        assert "permitting receipt deletion" in capsys.readouterr().out

    def it_names_a_near_miss_bypass_instead_of_honouring_it(repo, capsys):
        tmp_path, base_sha = repo
        (tmp_path / "packages" / "ts" / "e2e-attestations" / "old-branch.json").unlink()
        _git("add", "-A")
        _git("commit", "-m", "retire ts\n\nskip-receipt-deletion: package removed")
        head_sha = _head()

        assert run(base_sha, head_sha) == 1
        assert "is not the bypass line" in capsys.readouterr().out
