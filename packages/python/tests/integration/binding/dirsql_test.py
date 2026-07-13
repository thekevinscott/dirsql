"""Binding-tier tests (real core, real fs) for the DirSQL Python SDK."""

import json
import os

import pytest

from dirsql import DirSQL, Table


def describe_DirSQL():
    def describe_init():
        @pytest.mark.asyncio
        async def it_creates_instance_with_tables(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
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
            assert db is not None

        @pytest.mark.asyncio
        async def it_accepts_ignore_patterns(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                ignore=["**/def/**"],
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
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
            assert ids == {"abc"}

    def describe_default_config():
        @pytest.mark.asyncio
        async def it_serves_the_baked_in_files_table_with_no_config(tmp_path):
            # #603: a DirSQL with no `config` and no `tables` serves the
            # baked-in default `files` table (parity with the CLI's no-`-c`
            # default), not an empty index.
            (tmp_path / "readme.md").write_text("hello")
            db = DirSQL(str(tmp_path))
            await db.ready()
            rows = await db.query("SELECT basename FROM files")
            names = {r["basename"] for r in rows}
            assert "readme.md" in names

    def describe_query():
        @pytest.mark.asyncio
        async def it_returns_all_rows(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
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
            results = await db.query("SELECT * FROM comments")
            assert len(results) == 3

        @pytest.mark.asyncio
        async def it_returns_dicts_with_column_names(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
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
            results = await db.query(
                "SELECT author FROM comments WHERE body = 'first comment'"
            )
            assert len(results) == 1
            assert results[0]["author"] == "alice"

        @pytest.mark.asyncio
        async def it_filters_with_where_clause(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
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
            results = await db.query("SELECT * FROM comments WHERE id = 'abc'")
            assert len(results) == 2
            assert all(r["id"] == "abc" for r in results)

        @pytest.mark.asyncio
        async def it_excludes_internal_tracking_columns(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
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
            results = await db.query("SELECT * FROM comments LIMIT 1")
            assert len(results) == 1
            row = results[0]
            assert "_dirsql_file_path" not in row
            assert "_dirsql_row_index" not in row

        @pytest.mark.asyncio
        async def it_handles_integer_values(tmp_dir):
            os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
            with open(os.path.join(tmp_dir, "data", "counts.json"), "w") as f:
                json.dump({"name": "apples", "count": 42}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT, count INTEGER)",
                        glob="data/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apples"
            assert results[0]["count"] == 42

    def describe_multiple_tables():
        @pytest.mark.asyncio
        async def it_supports_multiple_table_definitions(tmp_dir):
            os.makedirs(os.path.join(tmp_dir, "posts"), exist_ok=True)
            os.makedirs(os.path.join(tmp_dir, "authors"), exist_ok=True)

            with open(os.path.join(tmp_dir, "posts", "hello.json"), "w") as f:
                json.dump({"title": "Hello World", "author_id": "1"}, f)

            with open(os.path.join(tmp_dir, "authors", "alice.json"), "w") as f:
                json.dump({"id": "1", "name": "Alice"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE posts (title TEXT, author_id TEXT)",
                        glob="posts/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                    Table(
                        ddl="CREATE TABLE authors (id TEXT, name TEXT)",
                        glob="authors/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()
            posts = await db.query("SELECT * FROM posts")
            authors = await db.query("SELECT * FROM authors")
            assert len(posts) == 1
            assert len(authors) == 1
            assert posts[0]["title"] == "Hello World"
            assert authors[0]["name"] == "Alice"

        @pytest.mark.asyncio
        async def it_supports_joins_across_tables(tmp_dir):
            os.makedirs(os.path.join(tmp_dir, "posts"), exist_ok=True)
            os.makedirs(os.path.join(tmp_dir, "authors"), exist_ok=True)

            with open(os.path.join(tmp_dir, "posts", "hello.json"), "w") as f:
                json.dump({"title": "Hello World", "author_id": "1"}, f)

            with open(os.path.join(tmp_dir, "authors", "alice.json"), "w") as f:
                json.dump({"id": "1", "name": "Alice"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE posts (title TEXT, author_id TEXT)",
                        glob="posts/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                    Table(
                        ddl="CREATE TABLE authors (id TEXT, name TEXT)",
                        glob="authors/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query(
                "SELECT posts.title, authors.name "
                "FROM posts JOIN authors ON posts.author_id = authors.id"
            )
            assert len(results) == 1
            assert results[0]["title"] == "Hello World"
            assert results[0]["name"] == "Alice"

    def describe_error_handling():
        @pytest.mark.asyncio
        async def it_raises_on_invalid_sql(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
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
            with pytest.raises(Exception):
                await db.query("NOT VALID SQL")

        @pytest.mark.asyncio
        async def it_rejects_write_statements_via_query(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "apple"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            for stmt in [
                "DELETE FROM items",
                "DROP TABLE items",
                "INSERT INTO items (name) VALUES ('evil')",
                "UPDATE items SET name = 'x'",
                "CREATE TABLE evil (id TEXT)",
                "ALTER TABLE items ADD COLUMN evil TEXT",
                "REPLACE INTO items (name) VALUES ('x')",
                "VACUUM",
            ]:
                with pytest.raises(Exception, match="(?i)read-only|writeforbidden"):
                    await db.query(stmt)

            results = await db.query("SELECT name FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apple"

        @pytest.mark.asyncio
        async def it_rejects_attach_and_creates_no_file(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "apple"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            target = os.path.join(tmp_dir, "attached.db")
            with pytest.raises(Exception, match="(?i)not authorized"):
                await db.query(f"ATTACH '{target}' AS ext")
            assert not os.path.exists(target)

            with pytest.raises(Exception, match="(?i)not authorized"):
                await db.query("DETACH ext")

            results = await db.query("SELECT name FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apple"

        @pytest.mark.asyncio
        async def it_cannot_read_external_db_via_attach(tmp_dir):
            import sqlite3

            secret = os.path.join(tmp_dir, "secret.db")
            conn = sqlite3.connect(secret)
            conn.execute("CREATE TABLE secrets (v TEXT)")
            conn.execute("INSERT INTO secrets (v) VALUES ('token')")
            conn.commit()
            conn.close()

            os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
            with open(os.path.join(tmp_dir, "data", "item.json"), "w") as f:
                json.dump({"name": "apple"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="data/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            with pytest.raises(Exception):
                await db.query(f"ATTACH '{secret}' AS ext")
            with pytest.raises(Exception):
                await db.query("SELECT v FROM ext.secrets")

        @pytest.mark.asyncio
        async def it_raises_on_invalid_ddl(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="NOT A CREATE TABLE",
                        glob="*.json",
                        on_file=lambda path: [],
                    ),
                ],
            )
            with pytest.raises(Exception):
                await db.ready()

        @pytest.mark.asyncio
        async def it_handles_empty_directory(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query("SELECT * FROM items")
            assert len(results) == 0

        @pytest.mark.asyncio
        async def it_handles_extract_returning_empty_list(tmp_dir):
            with open(os.path.join(tmp_dir, "skip.json"), "w") as f:
                json.dump({"ignore": True}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            results = await db.query("SELECT * FROM items")
            assert len(results) == 0

    def describe_extract_receives_path():
        @pytest.mark.asyncio
        async def it_passes_absolute_path(tmp_dir):
            with open(os.path.join(tmp_dir, "test.json"), "w") as f:
                json.dump({"val": 1}, f)

            captured = {}

            def on_file(path):
                captured["path"] = path
                captured["content"] = open(path, encoding="utf-8").read()
                return [{"val": 1}]

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE t (val INTEGER)",
                        glob="*.json",
                        on_file=on_file,
                    ),
                ],
            )
            await db.ready()
            await db.query("SELECT * FROM t")
            assert os.path.isabs(captured["path"])
            assert os.path.basename(captured["path"]) == "test.json"
            assert '"val"' in captured["content"]

    def describe_reserved_word_columns():
        @pytest.mark.asyncio
        async def it_round_trips_a_reserved_word_column(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "apple", "order": 7}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl='CREATE TABLE items (name TEXT, "order" INTEGER)',
                        glob="*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query('SELECT name, "order" FROM items')
            assert len(results) == 1
            assert results[0]["name"] == "apple"
            assert results[0]["order"] == 7

    def describe_relaxed_schema():
        @pytest.mark.asyncio
        async def it_ignores_extra_keys_by_default(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "apple", "color": "red", "weight": 150}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apple"
            assert "color" not in results[0]
            assert "weight" not in results[0]

        @pytest.mark.asyncio
        async def it_fills_missing_keys_with_null(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "apple"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT, color TEXT, count INTEGER)",
                        glob="*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apple"
            assert results[0]["color"] is None
            assert results[0]["count"] is None

        @pytest.mark.asyncio
        async def it_raises_on_extra_keys_in_strict_mode(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "apple", "color": "red"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                        strict=True,
                    ),
                ],
            )
            with pytest.raises(Exception):
                await db.ready()

        @pytest.mark.asyncio
        async def it_raises_on_missing_keys_in_strict_mode(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "apple"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT, color TEXT)",
                        glob="*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                        strict=True,
                    ),
                ],
            )
            with pytest.raises(Exception):
                await db.ready()

        @pytest.mark.asyncio
        async def it_allows_exact_match_in_strict_mode(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "apple", "color": "red"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT, color TEXT)",
                        glob="*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                        strict=True,
                    ),
                ],
            )
            await db.ready()
            results = await db.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apple"
            assert results[0]["color"] == "red"
