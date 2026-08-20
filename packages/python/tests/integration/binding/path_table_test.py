"""Binding-tier tests (real core) for path-tables in ``query()``.

A table name SQLite does not know, but which looks like a path, resolves to a
live glob scan of the index root. The logic lives entirely in the Rust core, so
the SDK inherits it for free -- these tests prove the inheritance is real
across the PyO3 boundary.
"""

import os

import pytest

from dirsql import DirSQL, Table


@pytest.fixture
def docs_dir(tmp_path):
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "a.md").write_text("alpha", encoding="utf-8")
    (tmp_path / "docs" / "b.md").write_text("bravo body", encoding="utf-8")
    (tmp_path / "docs" / "c.csv").write_text("x,y", encoding="utf-8")
    return str(tmp_path)


def _open(root):
    return DirSQL(
        root,
        tables=[
            Table(
                name="rows_csv",
                ddl="CREATE TABLE rows_csv (path TEXT)",
                glob="docs/*.csv",
                on_file=lambda path: [{"path": "docs/c.csv"}],
            )
        ],
    )


def describe_path_tables():
    @pytest.mark.asyncio
    async def it_scans_the_index_root_for_a_bare_dot_slash(docs_dir):
        rows = await _open(docs_dir).query("SELECT path FROM './'")

        assert sorted(r["path"] for r in rows) == [
            "docs/a.md",
            "docs/b.md",
            "docs/c.csv",
        ]

    @pytest.mark.asyncio
    async def it_scopes_the_scan_to_the_glob(docs_dir):
        rows = await _open(docs_dir).query("SELECT basename, size FROM './docs/*.md'")

        assert sorted(r["basename"] for r in rows) == ["a.md", "b.md"]
        assert {r["basename"]: r["size"] for r in rows}["b.md"] == 10

    @pytest.mark.asyncio
    async def it_returns_no_rows_when_nothing_matches(docs_dir):
        assert await _open(docs_dir).query("SELECT path FROM './docs/*.rst'") == []

    @pytest.mark.asyncio
    async def it_joins_a_path_table_against_a_named_table(docs_dir):
        rows = await _open(docs_dir).query(
            "SELECT p.basename FROM './docs/*.csv' AS p "
            "JOIN rows_csv AS r ON r.path = p.path"
        )

        assert [r["basename"] for r in rows] == ["c.csv"]

    @pytest.mark.asyncio
    async def it_still_resolves_a_real_table_by_name(docs_dir):
        rows = await _open(docs_dir).query("SELECT path FROM rows_csv")

        assert rows == [{"path": "docs/c.csv"}]

    @pytest.mark.asyncio
    async def it_hints_at_the_dot_slash_form_for_a_bare_glob(docs_dir):
        db = _open(docs_dir)

        with pytest.raises(Exception) as excinfo:
            await db.query("SELECT * FROM '**/*.md'")

        assert "did you mean './**/*.md'?" in str(excinfo.value)

    @pytest.mark.asyncio
    async def it_leaves_a_plain_typo_unchanged(docs_dir):
        db = _open(docs_dir)

        with pytest.raises(Exception) as excinfo:
            await db.query("SELECT * FROM usrs")

        message = str(excinfo.value)
        assert "no such table: usrs" in message
        assert "did you mean" not in message

    @pytest.mark.asyncio
    async def it_reads_the_filesystem_live(docs_dir):
        db = _open(docs_dir)
        await db.query("SELECT path FROM './docs/*.md'")

        with open(os.path.join(docs_dir, "docs", "d.md"), "w", encoding="utf-8") as fh:
            fh.write("delta")

        rows = await db.query("SELECT path FROM './docs/*.md'")
        assert "docs/d.md" in [r["path"] for r in rows]

    @pytest.mark.asyncio
    async def it_excludes_content_from_star_but_selects_it_by_name(docs_dir):
        db = _open(docs_dir)

        starred = await db.query("SELECT * FROM './docs/*.md'")
        assert "content" not in starred[0]

        named = await db.query(
            "SELECT basename, content FROM './docs/*.md' WHERE basename = 'a.md'"
        )
        assert named == [{"basename": "a.md", "content": "alpha"}]

    @pytest.mark.asyncio
    async def it_yields_null_content_for_a_non_utf8_file(docs_dir):
        with open(os.path.join(docs_dir, "docs", "logo.bin"), "wb") as fh:
            fh.write(b"\xff\xd8\xff\xe0\x00\x80\x90")

        rows = await _open(docs_dir).query("SELECT content FROM './docs/*.bin'")

        assert rows == [{"content": None}]
