"""Binding-tier tests: ``watch=False`` builds from one walk and never watches."""

import gc
import os
import sys

import pytest

from dirsql import DirSQL, Table


def _inotify_watches():
    count = 0
    for fd in os.listdir("/proc/self/fd"):
        try:
            if os.readlink(f"/proc/self/fd/{fd}") != "anon_inode:inotify":
                continue
            with open(f"/proc/self/fdinfo/{fd}") as info:
                count += sum(line.startswith("inotify wd:") for line in info)
        except OSError:
            continue
    return count


def _items_db(root, **kwargs):
    return DirSQL(
        root,
        tables=[
            Table(
                name="items",
                ddl="CREATE TABLE items (name TEXT)",
                glob="**/*.md",
                on_file=lambda path: [{"name": os.path.basename(path)}],
            ),
        ],
        **kwargs,
    )


def describe_watch_false():
    @pytest.mark.skipif(sys.platform != "linux", reason="counts inotify watches")
    @pytest.mark.asyncio
    async def it_reads_the_build_walk_without_an_inotify_watch(tmp_dir):
        os.makedirs(os.path.join(tmp_dir, "sub"))
        open(os.path.join(tmp_dir, "sub", "built.md"), "w").close()
        gc.collect()
        before = _inotify_watches()

        db = _items_db(tmp_dir, watch=False)
        await db.ready()
        rows = await db.query("SELECT name FROM items")

        assert rows == [{"name": "built.md"}]
        assert _inotify_watches() - before == 0

    @pytest.mark.asyncio
    async def it_raises_on_watch(tmp_dir):
        db = _items_db(tmp_dir, watch=False)
        await db.ready()
        with pytest.raises(RuntimeError, match="watching is off"):
            db.watch()

    @pytest.mark.asyncio
    async def it_raises_on_watch_before_ready(tmp_dir):
        db = _items_db(tmp_dir, watch=False)
        with pytest.raises(RuntimeError, match="watching is off"):
            db.watch()
        await db.ready()
