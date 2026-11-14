"""E2E: a long-lived ``DirSQL`` reads what is on disk when each query runs.

Regressions for the reported repros, against the installed build: a declared
table read with no watch running, a change written between ``ready()`` and
``watch()`` starting, and a query racing a consumed ``watch()``. Every query
runs the moment the write before it returns. No mocks: real binding, real
filesystem, real SQLite.
"""

from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path

import pytest

from dirsql import DirSQL, Table

pytestmark = pytest.mark.skipif(
    sys.platform != "linux",
    reason="a query reads a write at once only where it drains the inotify queue",
)

RACE_ROUNDS = 1000


def _turns(root: Path) -> Table:
    def on_file(path):
        return [{"id": Path(path).parent.relative_to(root).as_posix()}]

    return Table(
        name="turns",
        ddl="CREATE TABLE turns (id TEXT PRIMARY KEY)",
        glob="turns/*/a.txt",
        on_file=on_file,
    )


def _records() -> Table:
    def on_file(path):
        p = Path(path)
        return [{"name": p.name, "v": json.loads(p.read_text())["v"]}]

    return Table(
        name="t",
        ddl="CREATE TABLE t (name TEXT, v INTEGER)",
        glob="*.json",
        on_file=on_file,
    )


def _add_turn(root: Path, n: int) -> None:
    (root / f"turns/{n}").mkdir()
    (root / f"turns/{n}/a.txt").write_text("x")


async def _turn_ids(db: DirSQL) -> list[str]:
    return [row["id"] for row in await db.query("SELECT id FROM turns ORDER BY id")]


async def _records_of(db: DirSQL) -> list[tuple[str, int]]:
    rows = await db.query("SELECT name, v FROM t ORDER BY name")
    return [(row["name"], row["v"]) for row in rows]


async def _open_turns(root: Path) -> DirSQL:
    db = DirSQL(str(root), no_ignore=True, tables=[_turns(root)])
    await db.ready()
    return db


async def _open_records(root: Path) -> DirSQL:
    db = DirSQL(str(root), tables=[_records()])
    await db.ready()
    return db


def describe_a_declared_table_with_no_watch_running():
    @pytest.mark.asyncio
    async def it_reads_a_file_written_after_ready(tmp_path):
        (tmp_path / "turns/0").mkdir(parents=True)
        (tmp_path / "turns/0/a.txt").write_text("x")
        db = await _open_turns(tmp_path)
        _add_turn(tmp_path, 1)
        assert await _turn_ids(db) == ["turns/0", "turns/1"]

    @pytest.mark.asyncio
    async def it_reads_a_create_an_edit_and_a_delete(tmp_path):
        (tmp_path / "a.json").write_text(json.dumps({"v": 1}))
        db = await _open_records(tmp_path)
        assert await _records_of(db) == [("a.json", 1)]
        (tmp_path / "b.json").write_text(json.dumps({"v": 2}))
        (tmp_path / "a.json").write_text(json.dumps({"v": 10}))
        assert await _records_of(db) == [("a.json", 10), ("b.json", 2)]
        (tmp_path / "b.json").unlink()
        assert await _records_of(db) == [("a.json", 10)]


def describe_a_change_written_before_watch_starts():
    @pytest.mark.asyncio
    async def it_is_read_by_the_next_query_and_streamed_once(tmp_path):
        (tmp_path / "turns/0").mkdir(parents=True)
        (tmp_path / "turns/0/a.txt").write_text("x")
        db = await _open_turns(tmp_path)
        _add_turn(tmp_path, 1)
        assert await _turn_ids(db) == ["turns/0", "turns/1"]
        stream = db.watch()
        # Unread by any query, so only the watcher reports it, after every
        # change before it.
        _add_turn(tmp_path, 2)
        inserted = []
        async for event in stream:
            assert event.action == "insert", event
            inserted.append(event.row["id"])
            if event.row["id"] == "turns/2":
                break
        assert inserted == ["turns/1", "turns/2"]
        assert await _turn_ids(db) == ["turns/0", "turns/1", "turns/2"]


def describe_a_query_racing_a_consumed_watch():
    @pytest.mark.asyncio
    async def it_reads_every_write_before_it(tmp_path):
        (tmp_path / "turns").mkdir()
        db = await _open_turns(tmp_path)
        stream = db.watch()

        async def consume():
            async for _ in stream:
                pass

        task = asyncio.create_task(consume())
        misses = []
        for n in range(RACE_ROUNDS):
            _add_turn(tmp_path, n)
            if len(await db.query("SELECT id FROM turns")) != n + 1:
                misses.append(n)
        task.cancel()
        assert misses == []

    @pytest.mark.asyncio
    async def it_reads_a_create_with_a_watch_consumed(tmp_path):
        (tmp_path / "a.json").write_text(json.dumps({"v": 1}))
        db = await _open_records(tmp_path)
        stream = db.watch()

        async def consume():
            async for _ in stream:
                pass

        task = asyncio.create_task(consume())
        (tmp_path / "b.json").write_text(json.dumps({"v": 2}))
        assert await _records_of(db) == [("a.json", 1), ("b.json", 2)]
        task.cancel()


def describe_a_path_table():
    @pytest.mark.asyncio
    async def it_reads_a_create_an_edit_and_a_delete(tmp_path):
        (tmp_path / "a.json").write_text(json.dumps({"v": 1}))
        db = DirSQL(str(tmp_path), no_ignore=True)
        await db.ready()
        sql = "SELECT path, content FROM './*.json' ORDER BY path"

        async def read():
            return [(row["path"], row["content"]) for row in await db.query(sql)]

        assert await read() == [("a.json", '{"v": 1}')]
        (tmp_path / "b.json").write_text(json.dumps({"v": 2}))
        (tmp_path / "a.json").write_text(json.dumps({"v": 10}))
        assert await read() == [("a.json", '{"v": 10}'), ("b.json", '{"v": 2}')]
        (tmp_path / "b.json").unlink()
        assert await read() == [("a.json", '{"v": 10}')]
