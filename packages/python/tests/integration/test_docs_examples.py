"""Integration tests that mirror every code example in the docs.

Each test is named to match the doc page and section it verifies.
If a doc example changes and these tests break, the docs need updating (or vice versa).
"""

import json
import os

import pytest

from dirsql import DirSQL, Table


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _blog_dir(root):
    """Set up the blog directory structure used in getting-started.md."""
    posts_dir = os.path.join(root, "posts")
    authors_dir = os.path.join(root, "authors")
    os.makedirs(posts_dir, exist_ok=True)
    os.makedirs(authors_dir, exist_ok=True)

    with open(os.path.join(posts_dir, "hello.json"), "w") as f:
        json.dump({"title": "Hello World", "author": "alice"}, f)

    with open(os.path.join(posts_dir, "second.json"), "w") as f:
        json.dump({"title": "Second Post", "author": "bob"}, f)

    with open(os.path.join(authors_dir, "alice.json"), "w") as f:
        json.dump({"id": "alice", "name": "Alice"}, f)

    with open(os.path.join(authors_dir, "bob.json"), "w") as f:
        json.dump({"id": "bob", "name": "Bob"}, f)

    return root


def _blog_tables():
    """Return the table definitions from the getting-started example."""
    return [
        Table(
            ddl="CREATE TABLE posts (title TEXT, author TEXT)",
            glob="posts/*.json",
            extract=lambda path, content: [json.loads(content)],
        ),
        Table(
            ddl="CREATE TABLE authors (id TEXT, name TEXT)",
            glob="authors/*.json",
            extract=lambda path, content: [json.loads(content)],
        ),
    ]


# ---------------------------------------------------------------------------
# getting-started.md
# ---------------------------------------------------------------------------


def describe_getting_started():
    @pytest.mark.asyncio
    async def it_matches_getting_started_query_all_posts(tmp_dir):
        """Docs: db.query('SELECT * FROM posts') returns all posts."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        posts = await db.query("SELECT * FROM posts")
        titles = sorted([p["title"] for p in posts])
        assert titles == ["Hello World", "Second Post"]

    @pytest.mark.asyncio
    async def it_matches_getting_started_join_example(tmp_dir):
        """Docs: JOIN posts with authors on posts.author = authors.id."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        results = await db.query(
            "SELECT posts.title, authors.name "
            "FROM posts "
            "JOIN authors ON posts.author = authors.id"
        )
        result_map = {r["title"]: r["name"] for r in results}
        assert result_map == {
            "Hello World": "Alice",
            "Second Post": "Bob",
        }


# ---------------------------------------------------------------------------
# guide/tables.md
# ---------------------------------------------------------------------------


def describe_tables_guide():
    @pytest.mark.asyncio
    async def it_matches_tables_guide_single_object_json(tmp_dir):
        """Docs: extract=lambda path, content: [json.loads(content)] for single-object JSON."""
        os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
        with open(os.path.join(tmp_dir, "data", "item.json"), "w") as f:
            json.dump({"name": "widget", "value": 42}, f)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT, value INTEGER)",
                    glob="data/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM items")
        assert len(results) == 1
        assert results[0]["name"] == "widget"
        assert results[0]["value"] == 42

    @pytest.mark.asyncio
    async def it_matches_tables_guide_jsonl_extraction(tmp_dir):
        """Docs: one row per line for JSONL files."""
        os.makedirs(os.path.join(tmp_dir, "comments", "abc"), exist_ok=True)
        with open(os.path.join(tmp_dir, "comments", "abc", "index.jsonl"), "w") as f:
            f.write(json.dumps({"body": "first", "author": "alice"}) + "\n")
            f.write(json.dumps({"body": "second", "author": "bob"}) + "\n")

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE comments (body TEXT, author TEXT)",
                    glob="comments/**/index.jsonl",
                    extract=lambda path, content: [
                        json.loads(line) for line in content.splitlines()
                    ],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM comments")
        assert len(results) == 2
        authors = sorted([r["author"] for r in results])
        assert authors == ["alice", "bob"]

    @pytest.mark.asyncio
    async def it_matches_tables_guide_derive_from_path(tmp_dir):
        """Docs: extract values from the file path (os.path.dirname)."""
        os.makedirs(os.path.join(tmp_dir, "comments", "abc"), exist_ok=True)
        with open(os.path.join(tmp_dir, "comments", "abc", "index.jsonl"), "w") as f:
            f.write(json.dumps({"body": "hello"}) + "\n")

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE comments (id TEXT, body TEXT)",
                    glob="comments/**/index.jsonl",
                    extract=lambda path, content: [
                        {
                            "id": os.path.basename(os.path.dirname(path)),
                            "body": json.loads(line)["body"],
                        }
                        for line in content.splitlines()
                    ],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM comments")
        assert len(results) == 1
        assert results[0]["id"] == "abc"
        assert results[0]["body"] == "hello"

    @pytest.mark.asyncio
    async def it_matches_tables_guide_skip_draft_files(tmp_dir):
        """Docs: conditionally skip files by returning []."""
        with open(os.path.join(tmp_dir, "draft.json"), "w") as f:
            json.dump({"title": "Draft Post", "draft": True}, f)
        with open(os.path.join(tmp_dir, "published.json"), "w") as f:
            json.dump({"title": "Published Post", "draft": False}, f)

        def extract(path, content):
            data = json.loads(content)
            if data.get("draft"):
                return []
            return [data]

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE posts (title TEXT)",
                    glob="*.json",
                    extract=extract,
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM posts")
        assert len(results) == 1
        assert results[0]["title"] == "Published Post"

    @pytest.mark.asyncio
    async def it_matches_tables_guide_multiple_tables(tmp_dir):
        """Docs: multiple Table definitions with different globs."""
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
        await db.ready()
        posts = await db.query("SELECT * FROM posts")
        authors = await db.query("SELECT * FROM authors")
        assert len(posts) == 1
        assert len(authors) == 1

    @pytest.mark.asyncio
    async def it_matches_tables_guide_ignore_patterns(tmp_dir):
        """Docs: ignore parameter excludes paths from all tables."""
        os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
        os.makedirs(os.path.join(tmp_dir, "node_modules"), exist_ok=True)

        with open(os.path.join(tmp_dir, "data", "item.json"), "w") as f:
            json.dump({"name": "real"}, f)

        with open(os.path.join(tmp_dir, "node_modules", "dep.json"), "w") as f:
            json.dump({"name": "ignored"}, f)

        db = DirSQL(
            tmp_dir,
            ignore=["**/node_modules/**"],
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT)",
                    glob="**/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM items")
        assert len(results) == 1
        assert results[0]["name"] == "real"

    @pytest.mark.asyncio
    async def it_matches_tables_guide_typed_columns(tmp_dir):
        """Docs: typed columns (TEXT, REAL, INTEGER)."""
        os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
        with open(os.path.join(tmp_dir, "data", "metric.json"), "w") as f:
            json.dump({"name": "cpu", "value": 0.85, "count": 100}, f)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE metrics (name TEXT, value REAL, count INTEGER)",
                    glob="data/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM metrics")
        assert len(results) == 1
        assert results[0]["name"] == "cpu"
        assert results[0]["value"] == pytest.approx(0.85)
        assert results[0]["count"] == 100

    @pytest.mark.asyncio
    async def it_matches_tables_guide_constraints(tmp_dir):
        """Docs: DDL with constraints like PRIMARY KEY, NOT NULL."""
        os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
        with open(os.path.join(tmp_dir, "data", "item.json"), "w") as f:
            json.dump({"id": "abc", "name": "Widget"}, f)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
                    glob="data/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM items")
        assert len(results) == 1
        assert results[0]["id"] == "abc"
        assert results[0]["name"] == "Widget"

    @pytest.mark.asyncio
    async def it_matches_tables_guide_value_types(tmp_dir):
        """Docs: supported value types table (str, int, float, bool, None)."""
        with open(os.path.join(tmp_dir, "item.json"), "w") as f:
            json.dump(
                {
                    "text_val": "hello",
                    "int_val": 42,
                    "float_val": 3.14,
                    "bool_val": True,
                    "null_val": None,
                },
                f,
            )

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (text_val TEXT, int_val INTEGER, float_val REAL, bool_val INTEGER, null_val TEXT)",
                    glob="*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM items")
        assert len(results) == 1
        row = results[0]
        assert row["text_val"] == "hello"
        assert row["int_val"] == 42
        assert row["float_val"] == pytest.approx(3.14)
        assert row["bool_val"] == 1  # bool -> INTEGER 0/1
        assert row["null_val"] is None


# ---------------------------------------------------------------------------
# guide/querying.md
# ---------------------------------------------------------------------------


def describe_querying_guide():
    @pytest.mark.asyncio
    async def it_matches_querying_guide_select_all(tmp_dir):
        """Docs: SELECT * FROM comments."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        results = await db.query("SELECT * FROM posts")
        assert len(results) == 2

    @pytest.mark.asyncio
    async def it_matches_querying_guide_where_filter(tmp_dir):
        """Docs: SELECT * FROM comments WHERE author = 'alice'."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        results = await db.query("SELECT * FROM posts WHERE author = 'alice'")
        assert len(results) == 1
        assert results[0]["title"] == "Hello World"

    @pytest.mark.asyncio
    async def it_matches_querying_guide_aggregation(tmp_dir):
        """Docs: SELECT author, COUNT(*) as n FROM comments GROUP BY author."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        results = await db.query(
            "SELECT author, COUNT(*) as n FROM posts GROUP BY author"
        )
        assert len(results) == 2
        count_map = {r["author"]: r["n"] for r in results}
        assert count_map["alice"] == 1
        assert count_map["bob"] == 1

    @pytest.mark.asyncio
    async def it_matches_querying_guide_join(tmp_dir):
        """Docs: JOIN across tables."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        results = await db.query(
            "SELECT posts.title, authors.name "
            "FROM posts "
            "JOIN authors ON posts.author = authors.id"
        )
        assert len(results) == 2

    @pytest.mark.asyncio
    async def it_matches_querying_guide_return_format(tmp_dir):
        """Docs: query returns list of dicts keyed by column name."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        results = await db.query("SELECT title, author FROM posts")
        assert isinstance(results, list)
        assert all(isinstance(r, dict) for r in results)
        assert all("title" in r and "author" in r for r in results)

    @pytest.mark.asyncio
    async def it_matches_querying_guide_internal_columns_excluded(tmp_dir):
        """Docs: _dirsql_file_path and _dirsql_row_index excluded from SELECT *."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        results = await db.query("SELECT * FROM posts LIMIT 1")
        row = results[0]
        assert "_dirsql_file_path" not in row
        assert "_dirsql_row_index" not in row

    @pytest.mark.asyncio
    async def it_matches_querying_guide_error_handling(tmp_dir):
        """Docs: invalid SQL raises an exception."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        with pytest.raises(Exception):
            await db.query("NOT VALID SQL")

    @pytest.mark.asyncio
    async def it_matches_querying_guide_empty_results(tmp_dir):
        """Docs: queries matching no rows return an empty list."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()

        results = await db.query("SELECT * FROM posts WHERE author = 'nobody'")
        assert results == []


# ---------------------------------------------------------------------------
# guide/async.md
# ---------------------------------------------------------------------------


def describe_async_guide():
    @pytest.mark.asyncio
    async def it_matches_async_guide_basic_usage(tmp_dir):
        """Docs: DirSQL with ready() and query()."""
        os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
        with open(os.path.join(tmp_dir, "data", "a.json"), "w") as f:
            json.dump({"name": "low", "value": 5}, f)
        with open(os.path.join(tmp_dir, "data", "b.json"), "w") as f:
            json.dump({"name": "high", "value": 15}, f)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT, value INTEGER)",
                    glob="data/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()

        results = await db.query("SELECT * FROM items WHERE value > 10")
        assert len(results) == 1
        assert results[0]["name"] == "high"
        assert results[0]["value"] == 15

    @pytest.mark.asyncio
    async def it_matches_async_guide_ready_idempotent(tmp_dir):
        """Docs: ready() can be called multiple times safely."""
        os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
        with open(os.path.join(tmp_dir, "data", "item.json"), "w") as f:
            json.dump({"name": "test", "value": 1}, f)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT, value INTEGER)",
                    glob="data/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()
        await db.ready()

        results = await db.query("SELECT * FROM items")
        assert len(results) == 1

    @pytest.mark.asyncio
    async def it_matches_async_guide_count_query(tmp_dir):
        """Docs: await db.query('SELECT COUNT(*) as n FROM items')."""
        os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
        with open(os.path.join(tmp_dir, "data", "a.json"), "w") as f:
            json.dump({"name": "one", "value": 1}, f)
        with open(os.path.join(tmp_dir, "data", "b.json"), "w") as f:
            json.dump({"name": "two", "value": 2}, f)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT, value INTEGER)",
                    glob="data/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()

        results = await db.query("SELECT COUNT(*) as n FROM items")
        assert len(results) == 1
        assert results[0]["n"] == 2


# ---------------------------------------------------------------------------
# guide/watching.md
# ---------------------------------------------------------------------------


def describe_watching_guide():
    @pytest.mark.asyncio
    async def it_matches_watching_guide_insert_event(tmp_dir):
        """Docs: watch() yields insert events with action, table, row, file_path."""
        import asyncio

        # Pre-create directories so the watcher can monitor them
        os.makedirs(os.path.join(tmp_dir, "comments", "abc"), exist_ok=True)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                    glob="comments/**/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()

        events = []

        async def collect():
            async for event in db.watch():
                # Mid-write the watcher may deliver a spurious event (e.g. an
                # error if the file is still being written); only the insert
                # is meaningful here.
                if event.action != "insert":
                    continue
                events.append(event)
                if len(events) >= 1:
                    break

        task = asyncio.create_task(collect())
        await asyncio.sleep(0.3)

        with open(os.path.join(tmp_dir, "comments", "abc", "index.json"), "w") as f:
            json.dump({"id": "abc", "body": "new comment", "author": "alice"}, f)

        try:
            await asyncio.wait_for(task, timeout=5.0)
        except asyncio.TimeoutError:
            pytest.fail("Timed out waiting for insert event")

        assert len(events) >= 1
        assert events[0].action == "insert"
        assert events[0].table == "comments"
        assert events[0].row["id"] == "abc"
        assert events[0].row["body"] == "new comment"
        assert events[0].row["author"] == "alice"
        assert events[0].file_path is not None

    @pytest.mark.asyncio
    async def it_matches_watching_guide_delete_event(tmp_dir):
        """Docs: watch() yields delete events when a file is removed."""
        import asyncio

        os.makedirs(os.path.join(tmp_dir, "comments", "abc"), exist_ok=True)
        with open(os.path.join(tmp_dir, "comments", "abc", "index.json"), "w") as f:
            json.dump({"id": "abc", "body": "deleted comment", "author": "alice"}, f)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                    glob="comments/**/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()

        events = []

        async def collect():
            async for event in db.watch():
                if event.action != "delete":
                    continue
                events.append(event)
                if len(events) >= 1:
                    break

        task = asyncio.create_task(collect())
        await asyncio.sleep(0.3)

        os.remove(os.path.join(tmp_dir, "comments", "abc", "index.json"))

        try:
            await asyncio.wait_for(task, timeout=5.0)
        except asyncio.TimeoutError:
            pytest.fail("Timed out waiting for delete event")

        assert len(events) >= 1
        assert events[0].action == "delete"
        assert events[0].table == "comments"
        assert events[0].row["id"] == "abc"

    @pytest.mark.asyncio
    async def it_matches_watching_guide_update_event(tmp_dir):
        """Docs: watch() yields update events with row and old_row."""
        import asyncio

        os.makedirs(os.path.join(tmp_dir, "comments", "abc"), exist_ok=True)
        with open(os.path.join(tmp_dir, "comments", "abc", "index.json"), "w") as f:
            json.dump(
                {
                    "id": "abc",
                    "body": "original comment",
                    "author": "alice",
                },
                f,
            )

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                    glob="comments/**/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()

        events = []

        async def collect():
            async for event in db.watch():
                # An update may surface as a single "update" or as a
                # "delete"+"insert" pair. Filter out the unrelated "error"
                # events that can fire mid-write.
                if event.action not in ("update", "delete", "insert"):
                    continue
                events.append(event)
                if len(events) >= 1:
                    break

        task = asyncio.create_task(collect())
        await asyncio.sleep(0.3)

        with open(os.path.join(tmp_dir, "comments", "abc", "index.json"), "w") as f:
            json.dump({"id": "abc", "body": "edited comment", "author": "alice"}, f)

        try:
            await asyncio.wait_for(task, timeout=5.0)
        except asyncio.TimeoutError:
            pytest.fail("Timed out waiting for update event")

        assert len(events) >= 1
        # Could be update or delete+insert depending on implementation
        actions = {e.action for e in events}
        assert "update" in actions or ("delete" in actions and "insert" in actions)

    @pytest.mark.asyncio
    async def it_matches_watching_guide_error_event(tmp_dir):
        """Docs: watch() yields error events when extract fails."""
        import asyncio

        # Pre-create directory so the watcher can monitor it
        os.makedirs(os.path.join(tmp_dir, "comments"), exist_ok=True)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                    glob="comments/**/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()

        events = []

        async def collect():
            async for event in db.watch():
                if event.action != "error":
                    continue
                events.append(event)
                if len(events) >= 1:
                    break

        task = asyncio.create_task(collect())
        await asyncio.sleep(0.3)

        with open(os.path.join(tmp_dir, "comments", "bad.json"), "w") as f:
            f.write("not json at all")

        try:
            await asyncio.wait_for(task, timeout=5.0)
        except asyncio.TimeoutError:
            pytest.fail("Timed out waiting for error event")

        assert len(events) >= 1
        assert events[0].action == "error"
        assert events[0].error is not None


# ---------------------------------------------------------------------------
# api/index.md
# ---------------------------------------------------------------------------


def describe_api_reference():
    @pytest.mark.asyncio
    async def it_matches_api_reference_dirsql_constructor(tmp_dir):
        """Docs: DirSQL(root, *, tables, ignore=None)."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()
        results = await db.query("SELECT * FROM posts")
        assert len(results) == 2

    @pytest.mark.asyncio
    async def it_matches_api_reference_dirsql_query(tmp_dir):
        """Docs: db.query(sql) -> list[dict]."""
        root = _blog_dir(tmp_dir)
        db = DirSQL(root, tables=_blog_tables())
        await db.ready()
        results = await db.query("SELECT title FROM posts")
        assert isinstance(results, list)
        assert all(isinstance(r, dict) for r in results)

    def it_matches_api_reference_table_attributes(tmp_dir):
        """Docs: Table.ddl and Table.glob are read-only attributes."""
        table = Table(
            ddl="CREATE TABLE items (name TEXT)",
            glob="**/*.json",
            extract=lambda path, content: [json.loads(content)],
        )
        assert table.ddl == "CREATE TABLE items (name TEXT)"
        assert table.glob == "**/*.json"

    @pytest.mark.asyncio
    async def it_matches_api_reference_dirsql_async_constructor(tmp_dir):
        """Docs: DirSQL(root, *, tables, ignore=None) with async ready/query."""
        os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
        with open(os.path.join(tmp_dir, "data", "item.json"), "w") as f:
            json.dump({"name": "test"}, f)

        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT)",
                    glob="data/*.json",
                    extract=lambda path, content: [json.loads(content)],
                ),
            ],
        )
        await db.ready()
        results = await db.query("SELECT * FROM items")
        assert len(results) == 1
