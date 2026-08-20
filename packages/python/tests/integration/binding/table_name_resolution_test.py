"""Binding-tier tests (real core) for a quoted-identifier DDL.

A table's name is declared, but its ``ddl`` is bring-your-own -- including the
**quoted identifier** ORMs and schema tools emit. Quotes are SQL *delimiters*,
not part of the name, so SQLite records the bare identifier and the declared
``name`` matches it without dirsql reading the DDL text.
"""

import json
import os

import pytest

from dirsql import DirSQL, Table


def describe_quoted_identifier_ddl():
    @pytest.mark.asyncio
    async def it_registers_and_queries_a_quoted_ddl_table(jsonl_dir):
        db = DirSQL(
            jsonl_dir,
            tables=[
                Table(
                    name="comments",
                    ddl='CREATE TABLE "comments" (id TEXT, body TEXT, author TEXT)',
                    glob="comments/**/index.jsonl",
                    on_file=lambda path: [
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
