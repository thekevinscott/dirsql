"""Binding-tier tests: a write made right after watch() returns is observed."""

import asyncio
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


def describe_watch_live_on_return():
    @pytest.mark.asyncio
    async def it_sees_a_write_made_right_after_watch_returns(tmp_dir):
        db = _items_db(tmp_dir)
        await db.ready()

        stream = db.watch()
        _write_atomically(tmp_dir, "advice")

        try:
            name = await asyncio.wait_for(_first_insert(stream), timeout=5.0)
        except TimeoutError:
            pytest.fail("the write made right after watch() produced no event")

        assert name == "advice"
        assert await db.query("SELECT name FROM items") == [{"name": "advice"}]

    @pytest.mark.asyncio
    async def it_sees_a_write_made_after_watch_then_ready(tmp_dir):
        db = _items_db(tmp_dir)

        stream = db.watch()
        await db.ready()
        _write_atomically(tmp_dir, "advice")

        try:
            name = await asyncio.wait_for(_first_insert(stream), timeout=5.0)
        except TimeoutError:
            pytest.fail("the write made after watch() and ready() produced no event")

        assert name == "advice"
        assert await db.query("SELECT name FROM items") == [{"name": "advice"}]
