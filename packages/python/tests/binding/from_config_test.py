"""Binding-tier tests (real core, real fs) for DirSQL(config=).

Config-defined tables produce one row per matched file. Each row's columns
come from filesystem facts: glob path captures and stat virtuals (`_path`,
`_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`). Content
interpretation is intentionally out of scope; for that, register a
programmatic Table with your own extract function.
"""

import os
import tempfile

import pytest

from dirsql import DirSQL


@pytest.fixture
def config_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _write(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(content)


def describe_DirSQL_from_config():
    def describe_basic():
        @pytest.mark.asyncio
        async def it_produces_one_row_per_matched_file(config_dir):
            _write(os.path.join(config_dir, "items", "a.csv"), "anything")
            _write(os.path.join(config_dir, "items", "b.csv"), "anything")
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE items (_path TEXT, _basename TEXT)"
glob = "items/*.csv"
""",
            )

            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            await db.ready()
            results = await db.query(
                "SELECT _path, _basename FROM items ORDER BY _path"
            )
            assert len(results) == 2
            assert results[0]["_path"] == "items/a.csv"
            assert results[0]["_basename"] == "a.csv"
            assert results[1]["_path"] == "items/b.csv"
            assert results[1]["_basename"] == "b.csv"

    def describe_path_captures():
        @pytest.mark.asyncio
        async def it_injects_path_captures_into_rows(config_dir):
            _write(
                os.path.join(config_dir, "comments", "thread-1", "a.txt"),
                "hello",
            )
            _write(
                os.path.join(config_dir, "comments", "thread-2", "a.txt"),
                "world",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, _basename TEXT)"
glob = "comments/{thread_id}/*.txt"
""",
            )

            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            await db.ready()
            results = await db.query(
                "SELECT thread_id, _basename FROM comments ORDER BY thread_id"
            )
            assert len(results) == 2
            assert results[0]["thread_id"] == "thread-1"
            assert results[1]["thread_id"] == "thread-2"

    def describe_stat_virtuals():
        @pytest.mark.asyncio
        async def it_exposes_stat_virtuals(config_dir):
            body = "# title\nhello world\n"
            _write(os.path.join(config_dir, "docs", "readme.md"), body)
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE files (_path TEXT, _basename TEXT, _dir TEXT, _ext TEXT, _size INTEGER, _mtime INTEGER)"
glob = "docs/*.md"
""",
            )

            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            await db.ready()
            results = await db.query(
                "SELECT _path, _basename, _dir, _ext, _size, _mtime FROM files"
            )
            assert len(results) == 1
            r = results[0]
            assert r["_path"] == "docs/readme.md"
            assert r["_basename"] == "readme.md"
            assert r["_dir"] == "docs"
            assert r["_ext"] == "md"
            assert r["_size"] == len(body)
            assert isinstance(r["_mtime"], int)
            assert r["_mtime"] > 0

    def describe_ignore():
        @pytest.mark.asyncio
        async def it_respects_ignore_patterns(config_dir):
            _write(os.path.join(config_dir, "data", "good.json"), "{}")
            _write(
                os.path.join(config_dir, "data", "node_modules", "bad.json"),
                "{}",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[dirsql]
ignore = ["**/node_modules/**"]

[[table]]
ddl = "CREATE TABLE items (_path TEXT)"
glob = "data/**/*.json"
""",
            )

            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            await db.ready()
            results = await db.query("SELECT _path FROM items")
            assert len(results) == 1
            assert results[0]["_path"] == "data/good.json"

    def describe_multiple_tables():
        @pytest.mark.asyncio
        async def it_loads_multiple_tables(config_dir):
            _write(os.path.join(config_dir, "posts", "hello.txt"), "x")
            _write(os.path.join(config_dir, "authors", "alice.txt"), "x")
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE posts (_basename TEXT)"
glob = "posts/*.txt"

[[table]]
ddl = "CREATE TABLE authors (_basename TEXT)"
glob = "authors/*.txt"
""",
            )

            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            await db.ready()
            posts = await db.query("SELECT _basename FROM posts")
            authors = await db.query("SELECT _basename FROM authors")
            assert len(posts) == 1
            assert len(authors) == 1
            assert posts[0]["_basename"] == "hello.txt"
            assert authors[0]["_basename"] == "alice.txt"

    def describe_error_handling():
        @pytest.mark.asyncio
        async def it_raises_on_missing_config_file(config_dir):
            db = DirSQL(config=os.path.join(config_dir, "nonexistent.toml"))
            with pytest.raises(Exception):
                await db.ready()

        @pytest.mark.asyncio
        async def it_raises_on_invalid_toml(config_dir):
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                "this is not valid [[[",
            )
            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            with pytest.raises(Exception):
                await db.ready()

        @pytest.mark.asyncio
        async def it_raises_on_missing_ddl(config_dir):
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
glob = "*.json"
""",
            )
            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            with pytest.raises(Exception):
                await db.ready()

    def describe_query_after_config():
        @pytest.mark.asyncio
        async def it_supports_sql_queries_after_config_init(config_dir):
            _write(os.path.join(config_dir, "items", "apple.json"), "x")
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE items (name TEXT, _size INTEGER)"
glob = "items/{name}.json"
""",
            )

            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            await db.ready()
            results = await db.query("SELECT name FROM items WHERE name = 'apple'")
            assert len(results) == 1
            assert results[0]["name"] == "apple"

            results = await db.query("SELECT * FROM items LIMIT 1")
            assert "_dirsql_file_path" not in results[0]
            assert "_dirsql_row_index" not in results[0]
