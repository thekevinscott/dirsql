"""Binding-tier tests (real core, real fs) for DirSQL persistent on-disk cache."""

import json
import os
import sqlite3
import tempfile

import pytest

from dirsql import DirSQL, Table


def _write(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(content)


def _items_table(call_count_box):
    """Build an items table whose on_file callback bumps `call_count_box[0]` per call."""

    def on_file(path):
        call_count_box[0] += 1
        return [json.loads(open(path, encoding="utf-8").read())]

    return Table(
        ddl="CREATE TABLE items (name TEXT, price REAL)",
        glob="items/*.json",
        on_file=on_file,
    )


@pytest.fixture
def persist_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def describe_persist():
    def describe_cold_start():
        @pytest.mark.asyncio
        async def it_writes_cache_to_dotdirsql(persist_dir):
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
            # Warm start: on_file not invoked for the unchanged file.
            assert box2[0] == 0
            results = await db2.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "apple"

        @pytest.mark.asyncio
        async def it_leaves_the_cache_file_untouched(persist_dir):
            """An unchanged tree is a no-op: the cache is read, never rewritten."""
            _write(
                os.path.join(persist_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )

            box1 = [0]
            db1 = DirSQL(persist_dir, tables=[_items_table(box1)], persist=True)
            await db1.ready()
            del db1

            cache = os.path.join(persist_dir, ".dirsql", "cache.db")
            before = open(cache, "rb").read()

            box2 = [0]
            db2 = DirSQL(persist_dir, tables=[_items_table(box2)], persist=True)
            await db2.ready()
            del db2

            after = open(cache, "rb").read()
            assert len(after) == len(before), "an unchanged tree must not grow the cache"
            assert after == before, "an unchanged tree must not rewrite the cache"

    def describe_changed_file():
        @pytest.mark.asyncio
        async def it_reparses_changed_files(persist_dir):
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
                        on_file=lambda path: (
                            box2.__setitem__(0, box2[0] + 1)
                            or [
                                {
                                    **json.loads(open(path, encoding="utf-8").read()),
                                    "sku": "X",
                                }
                            ]
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
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    )
                ],
                persist=True,
            )
            await db.ready()
            results = await db.query("SELECT name FROM items")
            assert {r["name"] for r in results} == {"apple"}

    def describe_racy_window():
        @pytest.mark.asyncio
        async def it_hash_confirms_files_inside_racy_window(persist_dir):
            """When a cached file's mtime falls inside the racy window
            (mtime >= snapshot_ns), the reconcile must fall back to a content
            hash instead of trusting the stat tuple. Corrupt the cached hash
            so the hash check fails; the file must be re-parsed."""
            path = os.path.join(persist_dir, "items", "a.json")
            _write(path, json.dumps({"name": "apple", "price": 1.5}))

            box1 = [0]
            db1 = DirSQL(persist_dir, tables=[_items_table(box1)], persist=True)
            await db1.ready()
            assert box1[0] == 1
            del db1  # release any file handles before mutating cache.db

            cache = os.path.join(persist_dir, ".dirsql", "cache.db")
            conn = sqlite3.connect(cache)
            # Force this file into the racy window by zeroing snapshot_ns,
            # and corrupt its cached hash so the hash-confirm branch fails.
            conn.execute(
                "UPDATE _dirsql_files SET snapshot_ns = 0, content_hash = zeroblob(32)"
            )
            conn.commit()
            conn.close()

            box2 = [0]
            db2 = DirSQL(persist_dir, tables=[_items_table(box2)], persist=True)
            await db2.ready()
            assert box2[0] == 1
            results = await db2.query("SELECT name FROM items")
            assert results[0]["name"] == "apple"

    def describe_dirsql_version_bump():
        @pytest.mark.asyncio
        async def it_rebuilds_cache_when_dirsql_version_changes(persist_dir):
            _write(
                os.path.join(persist_dir, "items", "a.json"),
                json.dumps({"name": "apple", "price": 1.5}),
            )

            box1 = [0]
            db1 = DirSQL(persist_dir, tables=[_items_table(box1)], persist=True)
            await db1.ready()
            assert box1[0] == 1
            del db1

            cache = os.path.join(persist_dir, ".dirsql", "cache.db")
            conn = sqlite3.connect(cache)
            conn.execute(
                "UPDATE _dirsql_meta SET value = 'bogus-version' "
                "WHERE key = 'dirsql_version'"
            )
            conn.commit()
            conn.close()

            box2 = [0]
            db2 = DirSQL(persist_dir, tables=[_items_table(box2)], persist=True)
            await db2.ready()
            # Version mismatch forces a full rebuild; the file is re-parsed.
            assert box2[0] == 1

    def describe_custom_persist_path():
        @pytest.mark.asyncio
        async def it_honors_custom_persist_path(persist_dir):
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
            assert not os.path.exists(os.path.join(persist_dir, ".dirsql", "cache.db"))
