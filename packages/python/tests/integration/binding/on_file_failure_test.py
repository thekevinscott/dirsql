"""Binding-tier tests: any per-file ``on_file`` failure fails the build and the
query, with an error naming the file, the table and the original error. A hook
that raises and a row the table rejects follow the same rule.
"""

import os
import sys

import pytest

from dirsql import DirSQL, Table


def _write(tmp_dir, name, text):
    path = os.path.join(tmp_dir, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


def _db(tmp_dir, on_file, ddl="CREATE TABLE notes (id TEXT, n INTEGER)", **kw):
    return DirSQL(
        tmp_dir,
        tables=[Table(name="notes", ddl=ddl, glob="d/*.txt", on_file=on_file, **kw)],
    )


def _assert_names(err, *parts):
    message = str(err.value)
    for part in parts:
        assert part in message, message


def describe_on_file_failure():
    @pytest.mark.asyncio
    async def it_fails_ready_and_query_when_a_hook_raises(tmp_dir):
        _write(tmp_dir, "d/a.txt", "1")
        _write(tmp_dir, "d/b.txt", "oops")

        def on_file(path):
            if path.endswith("b.txt"):
                raise KeyError("missing-xyzzy")
            return [{"id": path, "n": 1}]

        db = _db(tmp_dir, on_file)
        with pytest.raises(RuntimeError) as err:
            await db.ready()
        _assert_names(err, "d/b.txt", "notes", "missing-xyzzy")
        with pytest.raises(RuntimeError) as err:
            await db.query("SELECT * FROM notes")
        _assert_names(err, "d/b.txt", "notes", "missing-xyzzy")

    @pytest.mark.asyncio
    async def it_fails_on_a_not_null_violation(tmp_dir):
        _write(tmp_dir, "d/a.txt", "1")
        db = _db(
            tmp_dir,
            lambda path: [{"id": path, "n": None}],
            ddl="CREATE TABLE notes (id TEXT, n INTEGER NOT NULL)",
        )
        with pytest.raises(RuntimeError) as err:
            await db.ready()
        _assert_names(err, "d/a.txt", "notes", "NOT NULL")

    @pytest.mark.asyncio
    async def it_fails_on_a_unique_violation(tmp_dir):
        _write(tmp_dir, "d/a.txt", "1")
        _write(tmp_dir, "d/b.txt", "1")
        db = _db(
            tmp_dir,
            lambda path: [{"id": "same", "n": 1}],
            ddl="CREATE TABLE notes (id TEXT UNIQUE, n INTEGER)",
        )
        with pytest.raises(RuntimeError) as err:
            await db.ready()
        _assert_names(err, "d/", "notes", "UNIQUE")

    @pytest.mark.asyncio
    async def it_fails_on_a_strict_rejection(tmp_dir):
        _write(tmp_dir, "d/a.txt", "1")
        db = _db(tmp_dir, lambda path: [{"nope-xyzzy": 1}], strict=True)
        with pytest.raises(RuntimeError) as err:
            await db.ready()
        _assert_names(err, "d/a.txt", "notes", "nope-xyzzy")

    @pytest.mark.asyncio
    @pytest.mark.skipif(
        sys.platform != "linux",
        reason="a query reads a write at once only where it drains the inotify queue",
    )
    async def it_fails_every_query_while_a_changed_file_fails(tmp_dir):
        _write(tmp_dir, "d/a.txt", "1")

        def on_file(path):
            with open(path) as f:
                return [{"id": "a", "n": int(f.read())}]

        db = _db(tmp_dir, on_file)
        assert await db.query("SELECT n FROM notes") == [{"n": 1}]

        _write(tmp_dir, "d/a.txt", "half-writ")
        for _ in range(3):
            with pytest.raises(RuntimeError) as err:
                await db.query("SELECT n FROM notes")
            _assert_names(err, "d/a.txt", "notes", "half-writ")

        _write(tmp_dir, "d/a.txt", "22")
        assert await db.query("SELECT n FROM notes") == [{"n": 22}]
        assert await db.query("SELECT n FROM notes") == [{"n": 22}]
