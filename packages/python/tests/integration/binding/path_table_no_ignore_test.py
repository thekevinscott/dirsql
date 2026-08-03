"""Binding-tier tests (real core, real fs) for the ``no_ignore`` opt-out.

Path-table scans respect ``.gitignore`` by default; ``no_ignore=True``
restores the full walk. The behavior lives in the shared Rust core -- these
tests prove the constructor opt-out crosses the PyO3 boundary.
"""

import pytest

from dirsql import DirSQL


@pytest.fixture
def gitignored_dir(tmp_path):
    (tmp_path / ".gitignore").write_text("ignored.md\n", encoding="utf-8")
    (tmp_path / "kept.md").write_text("kept", encoding="utf-8")
    (tmp_path / "ignored.md").write_text("ignored", encoding="utf-8")
    return str(tmp_path)


def describe_no_ignore():
    @pytest.mark.asyncio
    async def it_excludes_gitignored_files_by_default(gitignored_dir):
        rows = await DirSQL(gitignored_dir).query("SELECT path FROM './'")

        assert sorted(r["path"] for r in rows) == [".gitignore", "kept.md"]

    @pytest.mark.asyncio
    async def it_returns_gitignored_files_with_no_ignore(gitignored_dir):
        db = DirSQL(gitignored_dir, no_ignore=True)

        rows = await db.query("SELECT path FROM './'")

        assert sorted(r["path"] for r in rows) == [
            ".gitignore",
            "ignored.md",
            "kept.md",
        ]

    @pytest.mark.asyncio
    async def it_keeps_the_built_in_floor_under_no_ignore(gitignored_dir, tmp_path):
        (tmp_path / "node_modules").mkdir()
        (tmp_path / "node_modules" / "dep.js").write_text("x", encoding="utf-8")
        db = DirSQL(gitignored_dir, no_ignore=True)

        rows = await db.query("SELECT path FROM './'")

        assert "node_modules/dep.js" not in [r["path"] for r in rows]
