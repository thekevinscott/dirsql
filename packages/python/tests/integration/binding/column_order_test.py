"""Binding-tier tests: a query's rows are keyed in the SELECT list's order."""

import os

import pytest

from dirsql import DirSQL, Table

COLUMNS = ["a", "b", "c", "d", "e", "f", "g", "h"]


def _db(tmp_dir):
    with open(os.path.join(tmp_dir, "row.json"), "w") as f:
        f.write("{}")
    return DirSQL(
        tmp_dir,
        tables=[
            Table(
                name="t",
                ddl=f"CREATE TABLE t ({', '.join(COLUMNS)})",
                glob="*.json",
                on_file=lambda path: [{c: i for i, c in enumerate(COLUMNS)}],
            )
        ],
    )


def describe_query_column_order():
    @pytest.mark.asyncio
    async def it_keys_rows_in_the_select_list_order(tmp_dir):
        db = _db(tmp_dir)
        projection = ["f", "c", "h", "a", "e", "b", "g", "d"]

        rows = await db.query(f"SELECT {', '.join(projection)} FROM t")

        assert list(rows[0]) == projection

    @pytest.mark.asyncio
    async def it_follows_the_select_list_when_it_is_reversed(tmp_dir):
        db = _db(tmp_dir)
        projection = list(reversed(COLUMNS))

        rows = await db.query(f"SELECT {', '.join(projection)} FROM t")

        assert list(rows[0]) == projection

    @pytest.mark.asyncio
    async def it_keys_select_star_in_declaration_order(tmp_dir):
        db = _db(tmp_dir)
        rows = await db.query("SELECT * FROM t")

        assert list(rows[0]) == COLUMNS
