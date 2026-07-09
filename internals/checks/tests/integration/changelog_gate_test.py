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
    def it_fails_when_sdk_code_changes_without_a_changelog_entry(repo, capsys):
        tmp_path, base_sha = repo
        rust_dir = tmp_path / "packages" / "rust" / "src"
        rust_dir.mkdir(parents=True)
        (rust_dir / "lib.rs").write_text("// code\n")
        head_sha = _commit("add sdk code")

        assert run(base_sha, head_sha) == 1
        assert "SDK code changed" in capsys.readouterr().err

    def it_passes_when_the_changelog_gains_an_entry(repo, capsys):
        tmp_path, base_sha = repo
        rust_dir = tmp_path / "packages" / "rust" / "src"
        rust_dir.mkdir(parents=True)
        (rust_dir / "lib.rs").write_text("// code\n")
        (tmp_path / "CHANGELOG.md").write_text("## [Unreleased]\n- Added a thing\n")
        head_sha = _commit("add sdk code with changelog")

        assert run(base_sha, head_sha) == 0
        assert "updated with" in capsys.readouterr().out

    def it_passes_when_a_changelog_fragment_is_added(repo, capsys):
        tmp_path, base_sha = repo
        rust_dir = tmp_path / "packages" / "rust" / "src"
        rust_dir.mkdir(parents=True)
        (rust_dir / "lib.rs").write_text("// code\n")
        fragment_dir = tmp_path / "changelog.d"
        fragment_dir.mkdir()
        (fragment_dir / "claude-my-branch-abc123.changed.md").write_text(
            "**Changed a thing.**\n"
        )
        head_sha = _commit("add sdk code with fragment")

        assert run(base_sha, head_sha) == 0
        assert "fragment" in capsys.readouterr().out

    def it_passes_via_the_skip_changelog_trailer(repo, capsys):
        tmp_path, base_sha = repo
        rust_dir = tmp_path / "packages" / "rust" / "src"
        rust_dir.mkdir(parents=True)
        (rust_dir / "lib.rs").write_text("// code\n")
        _git("add", "-A")
        subprocess.run(
            [
                "git",
                "commit",
                "-m",
                "internal refactor\n\nskip-changelog: no observable change",
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        head_sha = subprocess.run(
            ["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True
        ).stdout.strip()

        assert run(base_sha, head_sha) == 0
        assert "Bypassing CHANGELOG check" in capsys.readouterr().out

    def it_passes_when_no_sdk_code_changed(repo, capsys):
        tmp_path, base_sha = repo
        (tmp_path / "README.md").write_text("hello again\n")
        head_sha = _commit("docs tweak")

        assert run(base_sha, head_sha) == 0
        assert "No SDK code changes detected" in capsys.readouterr().out
