"""Integration tests for DirSQL persistent on-disk cache."""

import json
import os
import tempfile

import pytest

from dirsql import DirSQL, Table


def _write(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(content)


def _items_table(call_count_box):
    """Build an items table whose extract callback bumps `call_count_box[0]` per call."""

    def extract(_path, content):
        call_count_box[0] += 1
        return [json.loads(content)]

    return Table(
        ddl="CREATE TABLE items (name TEXT, price REAL)",
        glob="items/*.json",
        extract=extract,
    )


@pytest.fixture
def persist_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def describe_persist():
    def describe_cold_start():
        @pytest.mark.asyncio
        async def it_writes_cache_to_dotdirsql(persist_dir):
            """A cold start with persist=True creates `.dirsql/cache.db`."""
            _write(
                os.path.join(persist_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )
            box = [0]
            db = DirSQL(persist_dir, tables=[_items_table(box)], persist=True)
            await db.ready()
            results = await db.query("SELECT * FROM items")
            assert len(results) == 1
            assert os.path.exists(os.path.join(persist_dir, ".dirsql", "cache.db"))

    def describe_warm_start():
        @pytest.mark.asyncio
        async def it_trusts_unchanged_files(persist_dir):
            """A warm start with persist=True does not re-parse unchanged files."""
            _write(
                os.path.join(persist_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )

            box1 = [0]
            db1 = DirSQL(persist_dir, tables=[_items_table(box1)], persist=True)
            await db1.ready()
            assert box1[0] == 1

            box2 = [0]
            db2 = DirSQL(persist_dir, tables=[_items_table(box2)], persist=True)
            await db2.ready()
            # Warm start: extract not invoked for the unchanged file.
            assert box2[0] == 0
            results = await db2.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apple"

    def describe_changed_file():
        @pytest.mark.asyncio
        async def it_reparses_changed_files(persist_dir):
            """A modified file is re-parsed on warm start."""
            path = os.path.join(persist_dir, "items", "a.json")
            _write(path, json.dumps({"name": "apple", "price": 1.5}))

            box1 = [0]
            db1 = DirSQL(persist_dir, tables=[_items_table(box1)], persist=True)
            await db1.ready()

            # Bump mtime far enough into the future to escape the racy window.
            import time

            time.sleep(0.05)
            _write(path, json.dumps({"name": "cherry", "price": 9.99}))
            future = time.time() + 5
            os.utime(path, (future, future))

            box2 = [0]
            db2 = DirSQL(persist_dir, tables=[_items_table(box2)], persist=True)
            await db2.ready()
            assert box2[0] == 1
            results = await db2.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "cherry"

    def describe_deleted_file():
        @pytest.mark.asyncio
        async def it_drops_rows_for_deleted_files(persist_dir):
            """Files removed between runs have their rows dropped."""
            a = os.path.join(persist_dir, "items", "a.json")
            b = os.path.join(persist_dir, "items", "b.json")
            _write(a, json.dumps({"name": "apple", "price": 1.5}))
            _write(b, json.dumps({"name": "banana", "price": 0.75}))

            box1 = [0]
            db1 = DirSQL(persist_dir, tables=[_items_table(box1)], persist=True)
            await db1.ready()

            os.remove(b)

            box2 = [0]
            db2 = DirSQL(persist_dir, tables=[_items_table(box2)], persist=True)
            await db2.ready()
            results = await db2.query("SELECT name FROM items")
            assert {r["name"] for r in results} == {"apple"}

    def describe_new_file():
        @pytest.mark.asyncio
        async def it_ingests_new_files(persist_dir):
            """A file added between runs is parsed on warm start."""
            _write(
                os.path.join(persist_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )

            box1 = [0]
            db1 = DirSQL(persist_dir, tables=[_items_table(box1)], persist=True)
            await db1.ready()

            _write(
                os.path.join(persist_dir, "items", "b.json"),
                json.dumps({"name": "banana", "price": 0.75}),
            )

            box2 = [0]
            db2 = DirSQL(persist_dir, tables=[_items_table(box2)], persist=True)
            await db2.ready()
            assert box2[0] == 1
            results = await db2.query("SELECT name FROM items ORDER BY name")
            assert [r["name"] for r in results] == ["apple", "banana"]

    def describe_glob_change():
        @pytest.mark.asyncio
        async def it_forces_full_rebuild_on_config_change(persist_dir):
            """Changing a glob/DDL invalidates the cache and re-parses everything."""
            _write(
                os.path.join(persist_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )

            box1 = [0]
            db1 = DirSQL(persist_dir, tables=[_items_table(box1)], persist=True)
            await db1.ready()

            # Change the DDL — this changes the glob_config_hash and forces a
            # full rebuild on the next startup.
            box2 = [0]
            db2 = DirSQL(
                persist_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT, price REAL, sku TEXT)",
                        glob="items/*.json",
                        extract=lambda _p, c: (
                            box2.__setitem__(0, box2[0] + 1)
                            or [{**json.loads(c), "sku": "X"}]
                        ),
                    )
                ],
                persist=True,
            )
            await db2.ready()
            assert box2[0] == 1
            results = await db2.query("SELECT * FROM items")
            assert results[0]["sku"] == "X"

    def describe_dirsql_excluded():
        @pytest.mark.asyncio
        async def it_excludes_dotdirsql_from_walk(persist_dir):
            """The reserved `.dirsql/` dir at the root is never indexed."""
            # Real data file:
            _write(
                os.path.join(persist_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )
            # A bogus file inside .dirsql that would otherwise match the glob
            # if the scanner walked into it:
            _write(
                os.path.join(persist_dir, ".dirsql", "items", "boom.json"),
                json.dumps({"name": "BOOM", "price": -1}),
            )

            db = DirSQL(
                persist_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT, price REAL)",
                        glob="**/*.json",
                        extract=lambda _p, c: [json.loads(c)],
                    )
                ],
                persist=True,
            )
            await db.ready()
            results = await db.query("SELECT name FROM items")
            assert {r["name"] for r in results} == {"apple"}

    def describe_custom_persist_path():
        @pytest.mark.asyncio
        async def it_honors_custom_persist_path(persist_dir):
            """`persist_path` overrides the default cache location."""
            _write(
                os.path.join(persist_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )
            custom = os.path.join(persist_dir, "elsewhere", "my-cache.sqlite")
            box = [0]
            db = DirSQL(
                persist_dir,
                tables=[_items_table(box)],
                persist=True,
                persist_path=custom,
            )
            await db.ready()
            assert os.path.exists(custom)
            assert not os.path.exists(
                os.path.join(persist_dir, ".dirsql", "cache.db")
            )
