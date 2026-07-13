"""Binding-tier red tests for #570: the per-file row seam is `on_file`, not `extract`.

One name for the seam on every surface: `on-file` in TOML, `on_file` in the
Python SDK. The old `extract` keyword is a hard break (no deprecation alias).
"""

import json
import os

import pytest

from dirsql import DirSQL, Table


def _rows(path):
    return [
        {
            "id": os.path.basename(os.path.dirname(path)),
            "body": row["body"],
            "author": row["author"],
        }
        for line in open(path, encoding="utf-8").read().splitlines()
        for row in [json.loads(line)]
    ]


def describe_table_on_file():
    @pytest.mark.asyncio
    async def it_builds_tables_via_the_on_file_keyword(jsonl_dir):
        db = DirSQL(
            jsonl_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                    glob="comments/**/index.jsonl",
                    on_file=_rows,
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT DISTINCT id FROM comments")
        assert {r["id"] for r in results} == {"abc", "def"}

    def it_rejects_the_old_extract_keyword():
        with pytest.raises(TypeError):
            Table(
                ddl="CREATE TABLE t (n INTEGER)",
                glob="**/*.json",
                extract=lambda path: [],
            )
