"""Binding-tier tests: a directory the watcher cannot watch fails every query.

Linux only: a directory without read permission is one inotify cannot watch.
"""

import os
import sys

import pytest

from dirsql import DirSQL, Table

pytestmark = pytest.mark.skipif(
    sys.platform != "linux" or os.geteuid() == 0,
    reason="inotify refuses to watch an unreadable directory, except for root",
)


def describe_a_directory_the_watcher_cannot_watch():
    @pytest.mark.asyncio
    async def it_fails_every_query_after_it_appears(tmp_dir):
        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    name="items",
                    ddl="CREATE TABLE items (name TEXT)",
                    glob="**/*.md",
                    on_file=lambda path: [{"name": path}],
                ),
            ],
        )
        await db.ready()
        assert await db.query("SELECT name FROM items") == []

        stream = db.watch()
        os.mkdir(os.path.join(tmp_dir, "locked"), 0o000)
        # Changes are applied in order, so this file's event follows the fault.
        open(os.path.join(tmp_dir, "after.md"), "w").close()
        async for event in stream:
            if event.action == "insert":
                break

        for _ in range(2):
            with pytest.raises(RuntimeError, match="Permission denied"):
                await db.query("SELECT name FROM items")
