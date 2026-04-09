"""Integration tests for DirSQL.from_config() and AsyncDirSQL.from_config()."""

import json
import os
import tempfile

import pytest

from dirsql import DirSQL, AsyncDirSQL


@pytest.fixture
def config_dir():
    """Create a temp dir with data files and a .dirsql.toml config."""
    with tempfile.TemporaryDirectory() as d:
        yield d


def _write(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(content)


def describe_DirSQL_from_config():
    def describe_basic():
        def it_loads_json_files_via_config(config_dir):
            """from_config parses a .dirsql.toml and indexes JSON files."""
            _write(
                os.path.join(config_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )
            _write(
                os.path.join(config_dir, "items", "b.json"),
                json.dumps({"name": "banana", "price": 0.75}),
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "items/*.json"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query("SELECT * FROM items ORDER BY name")
            assert len(results) == 2
            assert results[0]["name"] == "apple"
            assert results[0]["price"] == 1.5
            assert results[1]["name"] == "banana"

        def it_loads_csv_files_via_config(config_dir):
            """from_config handles CSV format."""
            _write(
                os.path.join(config_dir, "data.csv"),
                "name,count\napples,10\noranges,20\n",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE produce (name TEXT, count TEXT)"
glob = "*.csv"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query("SELECT * FROM produce ORDER BY name")
            assert len(results) == 2
            assert results[0]["name"] == "apples"

        def it_loads_jsonl_files_via_config(config_dir):
            """from_config handles JSONL format."""
            _write(
                os.path.join(config_dir, "events.jsonl"),
                json.dumps({"type": "click", "count": 5})
                + "\n"
                + json.dumps({"type": "view", "count": 100})
                + "\n",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE events (type TEXT, count INTEGER)"
glob = "*.jsonl"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query("SELECT * FROM events ORDER BY type")
            assert len(results) == 2
            assert results[0]["type"] == "click"
            assert results[0]["count"] == 5

    def describe_path_captures():
        def it_injects_path_captures_into_rows(config_dir):
            """Glob {name} placeholders become column values."""
            _write(
                os.path.join(config_dir, "comments", "thread-1", "index.jsonl"),
                json.dumps({"body": "hello", "author": "alice"}) + "\n",
            )
            _write(
                os.path.join(config_dir, "comments", "thread-2", "index.jsonl"),
                json.dumps({"body": "world", "author": "bob"}) + "\n",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT, author TEXT)"
glob = "comments/{thread_id}/index.jsonl"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query(
                "SELECT * FROM comments ORDER BY thread_id"
            )
            assert len(results) == 2
            assert results[0]["thread_id"] == "thread-1"
            assert results[0]["body"] == "hello"
            assert results[1]["thread_id"] == "thread-2"

    def describe_column_mapping():
        def it_applies_column_mapping(config_dir):
            """columns config maps dot-paths to SQL columns."""
            _write(
                os.path.join(config_dir, "people", "alice.json"),
                json.dumps(
                    {"metadata": {"author": {"name": "Alice"}}, "age": 30}
                ),
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE people (display_name TEXT, age INTEGER)"
glob = "people/*.json"

[table.columns]
display_name = "metadata.author.name"
age = "age"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query("SELECT * FROM people")
            assert len(results) == 1
            assert results[0]["display_name"] == "Alice"
            assert results[0]["age"] == 30

    def describe_each():
        def it_uses_each_to_navigate_into_arrays(config_dir):
            """each config navigates into nested arrays."""
            _write(
                os.path.join(config_dir, "catalog.json"),
                json.dumps(
                    {
                        "data": {
                            "items": [
                                {"name": "widget", "price": 9.99},
                                {"name": "gadget", "price": 19.99},
                            ]
                        }
                    }
                ),
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "catalog.json"
each = "data.items"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query("SELECT * FROM items ORDER BY name")
            assert len(results) == 2
            assert results[0]["name"] == "gadget"
            assert results[1]["name"] == "widget"

    def describe_ignore():
        def it_respects_ignore_patterns(config_dir):
            """Ignore patterns from config are applied."""
            _write(
                os.path.join(config_dir, "data", "good.json"),
                json.dumps({"val": 1}),
            )
            _write(
                os.path.join(config_dir, "data", "node_modules", "bad.json"),
                json.dumps({"val": 2}),
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[dirsql]
ignore = ["**/node_modules/**"]

[[table]]
ddl = "CREATE TABLE items (val INTEGER)"
glob = "data/**/*.json"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["val"] == 1

    def describe_multiple_tables():
        def it_loads_multiple_tables(config_dir):
            """Multiple [[table]] entries create multiple SQL tables."""
            _write(
                os.path.join(config_dir, "posts", "hello.json"),
                json.dumps({"title": "Hello"}),
            )
            _write(
                os.path.join(config_dir, "authors", "alice.json"),
                json.dumps({"name": "Alice"}),
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE posts (title TEXT)"
glob = "posts/*.json"

[[table]]
ddl = "CREATE TABLE authors (name TEXT)"
glob = "authors/*.json"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            posts = db.query("SELECT * FROM posts")
            authors = db.query("SELECT * FROM authors")
            assert len(posts) == 1
            assert len(authors) == 1
            assert posts[0]["title"] == "Hello"
            assert authors[0]["name"] == "Alice"

    def describe_error_handling():
        def it_raises_on_missing_config_file(config_dir):
            """from_config raises when the config file doesn't exist."""
            with pytest.raises(Exception):
                DirSQL.from_config(
                    os.path.join(config_dir, "nonexistent.toml")
                )

        def it_raises_on_invalid_toml(config_dir):
            """from_config raises on invalid TOML syntax."""
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                "this is not valid [[[",
            )
            with pytest.raises(Exception):
                DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))

        def it_raises_on_missing_ddl(config_dir):
            """from_config raises when a table entry is missing ddl."""
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
glob = "*.json"
""",
            )
            with pytest.raises(Exception):
                DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))

        def it_raises_on_unsupported_format(config_dir):
            """from_config raises when format cannot be inferred and none given."""
            _write(
                os.path.join(config_dir, "data.dat"),
                "some data",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.dat"
""",
            )
            with pytest.raises(Exception, match="[Ff]ormat|[Uu]nsupported"):
                DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))

    def describe_explicit_format():
        def it_uses_explicit_format_override(config_dir):
            """Explicit format in config overrides file extension inference."""
            # .txt file but explicitly marked as csv
            _write(
                os.path.join(config_dir, "data.txt"),
                "name,val\nfoo,1\n",
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE t (name TEXT, val TEXT)"
glob = "*.txt"
format = "csv"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query("SELECT * FROM t")
            assert len(results) == 1
            assert results[0]["name"] == "foo"

    def describe_query_after_config():
        def it_supports_sql_queries_after_config_init(config_dir):
            """Queries work the same way whether created via from_config or tables=."""
            _write(
                os.path.join(config_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )
            _write(
                os.path.join(config_dir, ".dirsql.toml"),
                """\
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "items/*.json"
""",
            )

            db = DirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
            results = db.query(
                "SELECT name FROM items WHERE price > 1.0"
            )
            assert len(results) == 1
            assert results[0]["name"] == "apple"

            # Internal columns should be hidden
            results = db.query("SELECT * FROM items LIMIT 1")
            assert "_dirsql_file_path" not in results[0]
            assert "_dirsql_row_index" not in results[0]


def describe_AsyncDirSQL_from_config():
    @pytest.mark.asyncio
    async def it_loads_config_async(config_dir):
        """AsyncDirSQL.from_config works like DirSQL.from_config but async."""
        _write(
            os.path.join(config_dir, "items", "a.json"),
            json.dumps({"name": "apple", "price": 1.5}),
        )
        _write(
            os.path.join(config_dir, ".dirsql.toml"),
            """\
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "items/*.json"
""",
        )

        db = AsyncDirSQL.from_config(os.path.join(config_dir, ".dirsql.toml"))
        await db.ready()
        results = await db.query("SELECT * FROM items")
        assert len(results) == 1
        assert results[0]["name"] == "apple"
