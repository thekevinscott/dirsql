"""Binding-tier tests (real core, real fs) for fan-out file->table matching.

A file matching N tables' globs populates all N tables; each table is an
independent view over the files matching its glob (#580).
"""

import os

import pytest

from dirsql import DirSQL, Table


def _write_fanout_file(tmp_dir):
    sub = os.path.join(tmp_dir, "data", "2401.00001")
    os.makedirs(sub, exist_ok=True)
    with open(os.path.join(sub, "metadata.json"), "w") as f:
        f.write("{}")
    return tmp_dir


def _table(name, glob, col, val):
    return Table(
        name=name,
        ddl=f"CREATE TABLE {name} ({col} TEXT)",
        glob=glob,
        on_file=lambda _path, col=col, val=val: [{col: val}],
    )


def describe_fanout():
    @pytest.mark.asyncio
    async def it_populates_both_tables_with_identical_globs(tmp_dir):
        _write_fanout_file(tmp_dir)
        db = DirSQL(
            tmp_dir,
            tables=[
                _table("ta", "data/*/metadata.json", "col_a", "A"),
                _table("tb", "data/*/metadata.json", "col_b", "B"),
            ],
        )
        await db.ready()

        a_rows = await db.query("SELECT col_a FROM ta")
        assert len(a_rows) == 1
        assert a_rows[0]["col_a"] == "A"

        b_rows = await db.query("SELECT col_b FROM tb")
        assert len(b_rows) == 1, "second-declared table must be populated"
        assert b_rows[0]["col_b"] == "B"

    @pytest.mark.asyncio
    async def it_populates_both_tables_with_overlapping_distinct_globs(tmp_dir):
        _write_fanout_file(tmp_dir)
        db = DirSQL(
            tmp_dir,
            tables=[
                _table("ta", "data/*/metadata.json", "col_a", "A"),
                _table("tb", "data/**/metadata.json", "col_b", "B"),
            ],
        )
        await db.ready()

        assert len(await db.query("SELECT col_a FROM ta")) == 1
        b_rows = await db.query("SELECT col_b FROM tb")
        assert len(b_rows) == 1, "second-declared table must be populated"
        assert b_rows[0]["col_b"] == "B"

    @pytest.mark.asyncio
    async def it_errors_when_a_placeholder_collides_with_a_column(tmp_dir):
        _write_fanout_file(tmp_dir)
        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    name="a",
                    ddl="CREATE TABLE a (id TEXT, col_a TEXT)",
                    glob="data/{id}/metadata.json",
                    on_file=lambda _path: [{"col_a": "A"}],
                ),
            ],
        )
        with pytest.raises(Exception, match="id"):
            await db.ready()
