"""Integration tests for robust table-name resolution (issue #204).

dirsql keeps ``ddl`` as the schema input (bring-your-own DDL, hand-written or
emitted by any ORM / schema tool). The only thing dirsql needs from the DDL is
the table *name*. The hand-rolled ``parse_table_name`` scanner mishandles a
**quoted identifier** -- the canonical shape emitted by Drizzle / SQLAlchemy /
Diesel / sea-query -- returning the name *with* the surrounding quotes, which
the downstream identifier validator then rejects.

#204 resolves the name via SQLite itself (execute the DDL, read the name back
from ``sqlite_master``). These tests assert the user-visible Python surface:
``Table.name`` resolves to the bare identifier, and a quoted-DDL table is
fully usable end to end.

RED today: ``Table(...).name`` is ``'"comments"'`` (quotes included), and
``DirSQL.ready()`` raises because that name fails identifier validation.
"""

import json
import os

import pytest

from dirsql import DirSQL, Table


def describe_quoted_identifier_ddl():
    def it_resolves_table_name_to_the_bare_identifier():
        """`Table.name` drops the quotes SQLite treats as delimiters."""
        table = Table(
            ddl='CREATE TABLE "comments" (id TEXT, body TEXT, author TEXT)',
            glob="comments/**/index.jsonl",
            extract=lambda path: [],
        )
        assert table.name == "comments"

    @pytest.mark.asyncio
    async def it_registers_and_queries_a_quoted_ddl_table(jsonl_dir):
        """A quoted-identifier DDL table is queryable by its bare name."""
        db = DirSQL(
            jsonl_dir,
            tables=[
                Table(
                    ddl='CREATE TABLE "comments" (id TEXT, body TEXT, author TEXT)',
                    glob="comments/**/index.jsonl",
                    extract=lambda path: [
                        {
                            "id": os.path.basename(os.path.dirname(path)),
                            "body": row["body"],
                            "author": row["author"],
                        }
                        for line in open(path, encoding="utf-8").read().splitlines()
                        for row in [json.loads(line)]
                    ],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT DISTINCT id FROM comments")
        ids = {r["id"] for r in results}
        assert ids == {"abc", "def"}
