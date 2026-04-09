"""Integration tests for the DirSQL Python SDK."""

import json
import os

import pytest

from dirsql import DirSQL, Table


def describe_DirSQL():
    def describe_init():
        def it_creates_instance_with_tables(jsonl_dir):
            """DirSQL can be initialized with a root path and table definitions."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            assert db is not None

        def it_accepts_ignore_patterns(jsonl_dir):
            """DirSQL accepts an ignore list to skip matching paths."""
            db = DirSQL(
                jsonl_dir,
                ignore=["**/def/**"],
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            # Only the "abc" directory should be indexed, not "def"
            results = db.query("SELECT DISTINCT id FROM comments")
            ids = {r["id"] for r in results}
            assert ids == {"abc"}

    def describe_query():
        def it_returns_all_rows(jsonl_dir):
            """query returns all indexed rows when no WHERE clause."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            results = db.query("SELECT * FROM comments")
            assert len(results) == 3

        def it_returns_dicts_with_column_names(jsonl_dir):
            """Each result row is a dict keyed by column name."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            results = db.query(
                "SELECT author FROM comments WHERE body = 'first comment'"
            )
            assert len(results) == 1
            assert results[0]["author"] == "alice"

        def it_filters_with_where_clause(jsonl_dir):
            """SQL WHERE clauses work correctly on indexed data."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            results = db.query("SELECT * FROM comments WHERE id = 'abc'")
            assert len(results) == 2
            assert all(r["id"] == "abc" for r in results)

        def it_excludes_internal_tracking_columns(jsonl_dir):
            """Internal _dirsql_* columns are not exposed in query results."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            results = db.query("SELECT * FROM comments LIMIT 1")
            assert len(results) == 1
            row = results[0]
            assert "_dirsql_file_path" not in row
            assert "_dirsql_row_index" not in row

        def it_handles_integer_values(tmp_dir):
            """Integer values in extracted data are preserved correctly."""
            os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
            with open(os.path.join(tmp_dir, "data", "counts.json"), "w") as f:
                json.dump({"name": "apples", "count": 42}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT, count INTEGER)",
                        glob="data/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            results = db.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apples"
            assert results[0]["count"] == 42

    def describe_multiple_tables():
        def it_supports_multiple_table_definitions(tmp_dir):
            """Multiple tables can be defined with different globs and extractors."""
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
                        extract=lambda path, content: [json.loads(content)],
                    ),
                    Table(
                        ddl="CREATE TABLE authors (id TEXT, name TEXT)",
                        glob="authors/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            posts = db.query("SELECT * FROM posts")
            authors = db.query("SELECT * FROM authors")
            assert len(posts) == 1
            assert len(authors) == 1
            assert posts[0]["title"] == "Hello World"
            assert authors[0]["name"] == "Alice"

        def it_supports_joins_across_tables(tmp_dir):
            """SQL JOINs work across different tables."""
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
                        extract=lambda path, content: [json.loads(content)],
                    ),
                    Table(
                        ddl="CREATE TABLE authors (id TEXT, name TEXT)",
                        glob="authors/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            results = db.query(
                "SELECT posts.title, authors.name "
                "FROM posts JOIN authors ON posts.author_id = authors.id"
            )
            assert len(results) == 1
            assert results[0]["title"] == "Hello World"
            assert results[0]["name"] == "Alice"

    def describe_error_handling():
        def it_raises_on_invalid_sql(jsonl_dir):
            """Invalid SQL raises an exception."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            with pytest.raises(Exception):
                db.query("NOT VALID SQL")

        def it_raises_on_invalid_ddl(tmp_dir):
            """Invalid DDL raises an exception during init."""
            with pytest.raises(Exception):
                DirSQL(
                    tmp_dir,
                    tables=[
                        Table(
                            ddl="NOT A CREATE TABLE",
                            glob="*.json",
                            extract=lambda path, content: [],
                        ),
                    ],
                )

        def it_handles_empty_directory(tmp_dir):
            """An empty directory produces zero rows."""
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            results = db.query("SELECT * FROM items")
            assert len(results) == 0

        def it_handles_extract_returning_empty_list(tmp_dir):
            """Extract function returning [] produces no rows for that file."""
            with open(os.path.join(tmp_dir, "skip.json"), "w") as f:
                json.dump({"ignore": True}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        extract=lambda path, content: [],
                    ),
                ],
            )
            results = db.query("SELECT * FROM items")
            assert len(results) == 0

    def describe_schema_mode():
        def it_ignores_extra_keys_by_default(tmp_dir):
            """Relaxed mode (default): extra keys returned by extract are silently dropped."""
            with open(os.path.join(tmp_dir, "data.json"), "w") as f:
                json.dump({"name": "alice", "age": 30, "extra": "ignored"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE t (name TEXT, age INTEGER)",
                        glob="*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            results = db.query("SELECT * FROM t")
            assert len(results) == 1
            assert results[0]["name"] == "alice"
            assert results[0]["age"] == 30
            assert "extra" not in results[0]

        def it_fills_missing_keys_with_null_by_default(tmp_dir):
            """Relaxed mode (default): missing keys become NULL."""
            with open(os.path.join(tmp_dir, "data.json"), "w") as f:
                json.dump({"name": "alice"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE t (name TEXT, age INTEGER)",
                        glob="*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            results = db.query("SELECT * FROM t")
            assert len(results) == 1
            assert results[0]["name"] == "alice"
            assert results[0]["age"] is None

        def it_raises_on_extra_keys_in_strict_mode(tmp_dir):
            """Strict mode: extra keys raise an error."""
            with open(os.path.join(tmp_dir, "data.json"), "w") as f:
                json.dump({"name": "alice", "extra": "bad"}, f)

            with pytest.raises(Exception, match="Schema error"):
                DirSQL(
                    tmp_dir,
                    tables=[
                        Table(
                            ddl="CREATE TABLE t (name TEXT)",
                            glob="*.json",
                            extract=lambda path, content: [json.loads(content)],
                            strict=True,
                        ),
                    ],
                )

        def it_raises_on_missing_keys_in_strict_mode(tmp_dir):
            """Strict mode: missing keys raise an error."""
            with open(os.path.join(tmp_dir, "data.json"), "w") as f:
                json.dump({"name": "alice"}, f)

            with pytest.raises(Exception, match="Schema error"):
                DirSQL(
                    tmp_dir,
                    tables=[
                        Table(
                            ddl="CREATE TABLE t (name TEXT, age INTEGER)",
                            glob="*.json",
                            extract=lambda path, content: [json.loads(content)],
                            strict=True,
                        ),
                    ],
                )

        def it_passes_strict_mode_with_exact_match(tmp_dir):
            """Strict mode: exact key match works fine."""
            with open(os.path.join(tmp_dir, "data.json"), "w") as f:
                json.dump({"name": "alice", "age": 30}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE t (name TEXT, age INTEGER)",
                        glob="*.json",
                        extract=lambda path, content: [json.loads(content)],
                        strict=True,
                    ),
                ],
            )
            results = db.query("SELECT * FROM t")
            assert len(results) == 1
            assert results[0]["name"] == "alice"
            assert results[0]["age"] == 30

    def describe_extract_receives_path_and_content():
        def it_passes_relative_path_and_string_content(tmp_dir):
            """Extract receives the file path (relative to root) and content as string."""
            with open(os.path.join(tmp_dir, "test.json"), "w") as f:
                json.dump({"val": 1}, f)

            captured = {}

            def extract(path, content):
                captured["path"] = path
                captured["content"] = content
                return [{"val": 1}]

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE t (val INTEGER)",
                        glob="*.json",
                        extract=extract,
                    ),
                ],
            )
            db.query("SELECT * FROM t")
            assert captured["path"] == "test.json"
            assert '"val"' in captured["content"]
