"""Binding-tier tests (real core, real fs) for DirSQL(config=).

Config-defined tables produce one row per matched file. Each row's columns
come from the table's `on-file` hook: a small `sh` command that derives the
stat facts (`path`, `basename`, `dir`, `ext`, `size`, `mtime`) from the file
and emits them as a JSON row. Content interpretation beyond that is out of
scope; for richer parsing, register a programmatic Table with your own on_file
function.
"""

import os
import tempfile

import pytest

from dirsql import DirSQL

# `on-file` hooks that emit the stat facts a row needs, derived from the
# file path (`{path}`) relative to the scan root (`{root}`). Emitting a
# superset of the DDL's columns is safe -- undeclared keys are dropped.
_HOOK_PATH = r"""on-file = '''sh -c 'rel=${1#"$2"/}; printf "[{\"path\":\"%s\"}]" "$rel"' sh {path} {root}'''"""
_HOOK_PATH_BASENAME = r"""on-file = '''sh -c 'rel=${1#"$2"/}; base=${1##*/}; printf "[{\"path\":\"%s\",\"basename\":\"%s\"}]" "$rel" "$base"' sh {path} {root}'''"""
_HOOK_STAT = r"""on-file = '''sh -c 'rel=${1#"$2"/}; base=${1##*/}; case "$rel" in */*) dir=${rel%/*};; *) dir="";; esac; ext=${base##*.}; [ "$ext" = "$base" ] && ext=""; size=$(wc -c < "$1" | tr -d " "); mtime=$(stat -c %Y "$1"); printf "[{\"path\":\"%s\",\"basename\":\"%s\",\"dir\":\"%s\",\"ext\":\"%s\",\"size\":%s,\"mtime\":%s}]" "$rel" "$base" "$dir" "$ext" "$size" "$mtime"' sh {path} {root}'''"""


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
name = "items"
ddl = "CREATE TABLE items (path TEXT, basename TEXT)"
glob = "items/*.csv"
"""
                + _HOOK_PATH_BASENAME
                + "\n",
            )

            db = DirSQL(
                root=config_dir, config=os.path.join(config_dir, ".dirsql.toml")
            )
            await db.ready()
            results = await db.query("SELECT path, basename FROM items ORDER BY path")
            assert len(results) == 2
            assert results[0]["path"] == "items/a.csv"
            assert results[0]["basename"] == "a.csv"
            assert results[1]["path"] == "items/b.csv"
            assert results[1]["basename"] == "b.csv"

    def describe_glob_placeholders():
        @pytest.mark.asyncio
        async def it_errors_when_a_placeholder_collides_with_a_column(config_dir):
            _write(
                os.path.join(config_dir, "comments", "thread-1", "a.txt"),
                "hello",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
name = "comments"
ddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"
glob = "comments/{thread_id}/*.txt"
"""
                + _HOOK_PATH_BASENAME
                + "\n",
            )

            db = DirSQL(
                root=config_dir, config=os.path.join(config_dir, ".dirsql.toml")
            )
            with pytest.raises(Exception, match="thread_id"):
                await db.ready()

        @pytest.mark.asyncio
        async def it_treats_a_non_colliding_placeholder_as_a_wildcard(config_dir):
            _write(
                os.path.join(config_dir, "comments", "thread-1", "a.txt"),
                "hello",
            )
            _write(
                os.path.join(config_dir, "comments", "thread-2", "b.txt"),
                "world",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
name = "comments"
ddl = "CREATE TABLE comments (path TEXT, basename TEXT)"
glob = "comments/{thread_id}/*.txt"
"""
                + _HOOK_PATH_BASENAME
                + "\n",
            )

            db = DirSQL(
                root=config_dir, config=os.path.join(config_dir, ".dirsql.toml")
            )
            await db.ready()
            results = await db.query("SELECT basename FROM comments ORDER BY basename")
            assert len(results) == 2
            assert results[0]["basename"] == "a.txt"
            assert results[1]["basename"] == "b.txt"
            assert "thread_id" not in results[0]

    def describe_stat_virtuals():
        @pytest.mark.asyncio
        async def it_exposes_stat_virtuals(config_dir):
            body = "# title\nhello world\n"
            _write(os.path.join(config_dir, "docs", "readme.md"), body)
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT, basename TEXT, dir TEXT, ext TEXT, size INTEGER, mtime INTEGER)"
glob = "docs/*.md"
"""
                + _HOOK_STAT
                + "\n",
            )

            db = DirSQL(
                root=config_dir, config=os.path.join(config_dir, ".dirsql.toml")
            )
            await db.ready()
            results = await db.query(
                "SELECT path, basename, dir, ext, size, mtime FROM files"
            )
            assert len(results) == 1
            r = results[0]
            assert r["path"] == "docs/readme.md"
            assert r["basename"] == "readme.md"
            assert r["dir"] == "docs"
            assert r["ext"] == "md"
            assert r["size"] == len(body)
            assert isinstance(r["mtime"], int)
            assert r["mtime"] > 0

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
name = "items"
ddl = "CREATE TABLE items (path TEXT)"
glob = "data/**/*.json"
"""
                + _HOOK_PATH
                + "\n",
            )

            db = DirSQL(
                root=config_dir, config=os.path.join(config_dir, ".dirsql.toml")
            )
            await db.ready()
            results = await db.query("SELECT path FROM items")
            assert len(results) == 1
            assert results[0]["path"] == "data/good.json"

    def describe_multiple_tables():
        @pytest.mark.asyncio
        async def it_loads_multiple_tables(config_dir):
            _write(os.path.join(config_dir, "posts", "hello.txt"), "x")
            _write(os.path.join(config_dir, "authors", "alice.txt"), "x")
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
name = "posts"
ddl = "CREATE TABLE posts (basename TEXT)"
glob = "posts/*.txt"
"""
                + _HOOK_PATH_BASENAME
                + """

[[table]]
name = "authors"
ddl = "CREATE TABLE authors (basename TEXT)"
glob = "authors/*.txt"
"""
                + _HOOK_PATH_BASENAME
                + "\n",
            )

            db = DirSQL(
                root=config_dir, config=os.path.join(config_dir, ".dirsql.toml")
            )
            await db.ready()
            posts = await db.query("SELECT basename FROM posts")
            authors = await db.query("SELECT basename FROM authors")
            assert len(posts) == 1
            assert len(authors) == 1
            assert posts[0]["basename"] == "hello.txt"
            assert authors[0]["basename"] == "alice.txt"

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
        async def it_raises_on_a_hookless_table(config_dir):
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT, size INTEGER)"
glob = "**/*.md"
""",
            )
            db = DirSQL(config=os.path.join(config_dir, ".dirsql.toml"))
            with pytest.raises(Exception, match="FROM './'"):
                await db.ready()

        @pytest.mark.asyncio
        async def it_raises_on_missing_ddl(config_dir):
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
name = "t"
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
name = "items"
ddl = "CREATE TABLE items (basename TEXT, size INTEGER)"
glob = "items/*.json"
"""
                + _HOOK_PATH_BASENAME
                + "\n",
            )

            db = DirSQL(
                root=config_dir, config=os.path.join(config_dir, ".dirsql.toml")
            )
            await db.ready()
            results = await db.query(
                "SELECT basename FROM items WHERE basename = 'apple.json'"
            )
            assert len(results) == 1
            assert results[0]["basename"] == "apple.json"

            results = await db.query("SELECT * FROM items LIMIT 1")
            assert "_dirsql_file_path" not in results[0]
            assert "_dirsql_row_index" not in results[0]
