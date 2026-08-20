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


def _write(path, text="// code\n"):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def _gate(repo, base_sha, head_sha):
    return subprocess.run(
        [_cli(), "changelog-gate", "--base-sha", base_sha, "--head-sha", head_sha],
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
    base_sha = _commit(tmp_path, "initial commit")
    return tmp_path, base_sha


def describe_dirsql_checks_changelog_gate():
    def it_exits_nonzero_when_package_source_changes_without_a_fragment(repo):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        head_sha = _commit(tmp_path, "add rust code")

        proc = _gate(tmp_path, base_sha, head_sha)
        assert proc.returncode == 1, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "packages/rust has code changes" in proc.stdout

    def it_exits_zero_when_a_colocated_fragment_is_added(repo):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        _write(
            tmp_path / "packages" / "rust" / "changelog.d" / "2026-07-13-fix.md",
            "**Changed a thing.**\n",
        )
        head_sha = _commit(tmp_path, "add rust code with fragment")

        proc = _gate(tmp_path, base_sha, head_sha)
        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"

    def it_exits_nonzero_when_the_fragment_is_in_another_package(repo):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        _write(
            tmp_path / "packages" / "ts" / "changelog.d" / "2026-07-13-unrelated.md",
            "**Changed.**\n",
        )
        head_sha = _commit(tmp_path, "rust change, ts fragment")

        proc = _gate(tmp_path, base_sha, head_sha)
        assert proc.returncode == 1, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "packages/rust has code changes" in proc.stdout

    def it_exits_nonzero_when_plugin_source_changes_without_a_fragment(repo):
        tmp_path, base_sha = repo
        _write(
            tmp_path / "plugins" / "dirsql-plugin-embeddings" / "src" / "x.py",
            "# code\n",
        )
        head_sha = _commit(tmp_path, "add plugin code")

        proc = _gate(tmp_path, base_sha, head_sha)
        assert proc.returncode == 1, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "plugins/dirsql-plugin-embeddings has code changes" in proc.stdout

    def it_exits_zero_when_a_colocated_plugin_fragment_is_added(repo):
        tmp_path, base_sha = repo
        plugin = tmp_path / "plugins" / "dirsql-plugin-embeddings"
        _write(plugin / "src" / "x.py", "# code\n")
        _write(plugin / "changelog.d" / "2026-08-12-fix.md", "**Fixed** a thing.\n")
        head_sha = _commit(tmp_path, "add plugin code with fragment")

        proc = _gate(tmp_path, base_sha, head_sha)
        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"

    def it_exits_zero_via_the_skip_changelog_line(repo):
        tmp_path, base_sha = repo
        _write(tmp_path / "packages" / "rust" / "src" / "lib.rs")
        head_sha = _commit(tmp_path, "internal refactor\n\nskip-changelog: no change")

        proc = _gate(tmp_path, base_sha, head_sha)
        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "bypassing changelog enforcement" in proc.stdout
