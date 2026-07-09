"""Binding-tier tests (real core) for table-name resolution from DDL.

The only thing dirsql needs from a (bring-your-own) DDL is the table *name*,
which must also resolve for a **quoted identifier** -- the canonical shape
emitted by ORMs / schema tools. ``Table.name`` must be the bare identifier,
and a quoted-DDL table fully usable end to end.
"""

import json
import os

import pytest

from dirsql import DirSQL, Table


def describe_quoted_identifier_ddl():
    def it_resolves_table_name_to_the_bare_identifier():
        table = Table(
            ddl='CREATE TABLE "comments" (id TEXT, body TEXT, author TEXT)',
            glob="comments/**/index.jsonl",
            extract=lambda path: [],
        )
        assert table.name == "comments"

    @pytest.mark.asyncio
    async def it_registers_and_queries_a_quoted_ddl_table(jsonl_dir):
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
