"""Integration tests for the changelog-gate check against a real git repo.

Exercises `gate.run` with its default collaborators (the real `git_ops`
functions, real `git` subprocess calls) over a scratch repo fixture -- never
the packaged `dirsql-checks` CLI (that's the e2e tier).
"""

from __future__ import annotations

import os
import subprocess

import pytest

from checks.changelog_gate.gate import run


def _git(*args: str) -> None:
    subprocess.run(["git", *args], check=True, capture_output=True, text=True)


def _commit(message: str) -> str:
    _git("add", "-A")
    _git("commit", "-m", message)
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True
    )
    return result.stdout.strip()


def _write(path, text="// code\n"):
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
        base_sha = _commit("initial commit")
        yield tmp_path, base_sha
    finally:
        os.chdir(original_cwd)


def describe_run_against_a_real_repo():
    def it_fails_when_package_source_changes_without_a_fragment(repo, capsys):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        head_sha = _commit("add rust code")

        assert run(base_sha, head_sha) == 1
        assert "packages/rust has code changes" in capsys.readouterr().out

    def it_passes_when_a_colocated_fragment_is_added(repo, capsys):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        _write(
            tmp_path / "packages" / "rust" / "changelog.d" / "2026-07-13-fix.md",
            "**Changed a thing.**\n",
        )
        head_sha = _commit("add rust code with fragment")

        assert run(base_sha, head_sha) == 0

    def it_fails_when_the_fragment_is_in_another_package(repo, capsys):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        _write(
            tmp_path / "packages" / "ts" / "changelog.d" / "2026-07-13-unrelated.md",
            "**Changed.**\n",
        )
        head_sha = _commit("rust change, ts fragment")

        assert run(base_sha, head_sha) == 1
        assert "packages/rust has code changes" in capsys.readouterr().out

    def it_accepts_a_migrations_fragment(repo, capsys):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        _write(
            tmp_path / "packages" / "rust" / "migrations.d" / "2026-07-13-break.md",
            "### break\n",
        )
        head_sha = _commit("rust change with migration")

        assert run(base_sha, head_sha) == 0

    def it_flags_a_malformed_fragment_filename(repo, capsys):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "changelog.d" / "notes.md", "x\n")
        head_sha = _commit("bad fragment name")

        assert run(base_sha, head_sha) == 1
        assert "fragment filenames must match" in capsys.readouterr().out

    def it_passes_via_the_skip_changelog_line(repo, capsys):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        _git("add", "-A")
        subprocess.run(
            ["git", "commit", "-m", "internal refactor\n\nskip-changelog: no change"],
            check=True,
            capture_output=True,
            text=True,
        )
        head_sha = subprocess.run(
            ["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True
        ).stdout.strip()

        assert run(base_sha, head_sha) == 0
        assert "bypassing changelog enforcement" in capsys.readouterr().out

    def it_passes_when_no_package_source_changed(repo, capsys):
        tmp_path, base_sha = repo
        (tmp_path / "README.md").write_text("hello again\n")
        head_sha = _commit("docs tweak")

        assert run(base_sha, head_sha) == 0
        assert "No package source changed" in capsys.readouterr().out
