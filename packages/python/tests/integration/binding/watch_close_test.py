"""Binding-tier tests: a closed or dropped watch() stream holds no events."""

import asyncio
import gc
import json
import os

import pytest

from dirsql import DirSQL, Table


def _items_db(root):
    return DirSQL(
        root,
        tables=[
            Table(
                name="items",
                ddl="CREATE TABLE items (name TEXT)",
                glob="**/*.json",
                on_file=lambda path: [json.loads(open(path, encoding="utf-8").read())],
            ),
        ],
    )


def _write_atomically(root, name):
    final = os.path.join(root, f"{name}.json")
    tmp = final + ".tmp"
    with open(tmp, "w") as f:
        json.dump({"name": name}, f)
    os.replace(tmp, final)


async def _first_insert(stream):
    async for event in stream:
        if event.action == "insert":
            return event.row["name"]


async def _indexed(db, name):
    while {"name": name} not in await db.query("SELECT name FROM items"):
        await asyncio.sleep(0.01)


def describe_watch_close():
    @pytest.mark.asyncio
    async def it_holds_no_events_between_a_closed_stream_and_the_next_watch(tmp_dir):
        db = _items_db(tmp_dir)
        await db.ready()
        async with db.watch() as events:
            _write_atomically(tmp_dir, "first")
            assert await _first_insert(events) == "first"

        _write_atomically(tmp_dir, "unwatched")
        await _indexed(db, "unwatched")

        async with db.watch() as events:
            _write_atomically(tmp_dir, "second")
            assert await _first_insert(events) == "second"

    @pytest.mark.asyncio
    async def it_holds_no_events_between_a_dropped_stream_and_the_next_watch(tmp_dir):
        db = _items_db(tmp_dir)
        await db.ready()
        stream = db.watch()
        _write_atomically(tmp_dir, "first")
        assert await _first_insert(stream) == "first"

        del stream
        gc.collect()
        _write_atomically(tmp_dir, "unwatched")
        await _indexed(db, "unwatched")

        stream = db.watch()
        _write_atomically(tmp_dir, "second")
        assert await _first_insert(stream) == "second"

    @pytest.mark.asyncio
    async def it_ends_a_consumer_waiting_in_another_task(tmp_dir):
        db = _items_db(tmp_dir)
        await db.ready()
        stream = db.watch()

        async def consume():
            return [event async for event in stream]

        consumer = asyncio.ensure_future(consume())
        await asyncio.sleep(0.05)

        await stream.aclose()

        assert await consumer == []

    @pytest.mark.asyncio
    async def it_keeps_an_open_stream_live_when_another_closes(tmp_dir):
        db = _items_db(tmp_dir)
        await db.ready()
        kept = db.watch()

        async with db.watch() as closing:
            _write_atomically(tmp_dir, "first")
            assert await _first_insert(closing) == "first"

        _write_atomically(tmp_dir, "second")
        assert await _first_insert(kept) == "first"
        assert await _first_insert(kept) == "second"
