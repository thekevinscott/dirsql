"""Binding-tier tests: while a child references no parent, every query
raises the foreign-key error, until the parent appears or the child goes.
"""

import asyncio
import sys
from pathlib import Path

import pytest

from dirsql import DirSQL, Table

pytestmark = pytest.mark.skipif(sys.platform != "linux", reason="Linux only")


def _tables():
    return [
        Table(
            name="parent",
            ddl="CREATE TABLE parent (id TEXT PRIMARY KEY)",
            glob="parents/*.txt",
            on_file=lambda path: [{"id": Path(path).stem}],
        ),
        Table(
            name="child",
            ddl="CREATE TABLE child (id TEXT, parent TEXT REFERENCES parent(id))",
            glob="children/*.txt",
            on_file=lambda path: [
                {"id": Path(path).stem, "parent": Path(path).read_text()}
            ],
        ),
    ]


async def _settle(db, sql, raises):
    """Wait for the index to catch up: until ``sql`` raises, or succeeds."""
    for _ in range(1000):
        try:
            await db.query(sql)
            if not raises:
                return
        except RuntimeError:
            if raises:
                return
        await asyncio.sleep(0.01)


async def _with_orphan(root):
    (root / "parents").mkdir()
    (root / "children").mkdir()
    db = DirSQL(str(root), tables=_tables())
    await db.ready()
    (root / "children" / "lost.txt").write_text("p1")
    await _settle(db, "SELECT id FROM child", raises=True)
    return db


async def _assert_raises(db):
    with pytest.raises(RuntimeError) as err:
        await db.query("SELECT id FROM child")
    message = str(err.value)
    assert "foreign key violation" in message, message
    assert "children/lost.txt" in message, message


def describe_a_child_with_no_parent():
    @pytest.mark.asyncio
    async def it_fails_every_query_until_the_parent_is_written(tmp_path):
        db = await _with_orphan(tmp_path)
        await _assert_raises(db)
        await _assert_raises(db)

        (tmp_path / "parents" / "p1.txt").write_text("")
        sql = (
            "SELECT c.id AS child, p.id AS parent"
            " FROM child c JOIN parent p ON p.id = c.parent"
        )
        await _settle(db, sql, raises=False)

        assert await db.query(sql) == [{"child": "lost", "parent": "p1"}]

    @pytest.mark.asyncio
    async def it_clears_when_the_child_is_deleted(tmp_path):
        db = await _with_orphan(tmp_path)
        await _assert_raises(db)

        (tmp_path / "children" / "lost.txt").unlink()
        await _settle(db, "SELECT id FROM child", raises=False)

        assert await db.query("SELECT id FROM child") == []
