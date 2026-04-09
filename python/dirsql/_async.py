"""Async wrapper for DirSQL."""

import asyncio

from dirsql._dirsql import DirSQL


class _WatchStream:
    """Async iterator that polls for file events."""

    def __init__(self, db):
        self._db = db
        self._started = False
        self._buffer = []

    def __aiter__(self):
        return self

    async def __anext__(self):
        if not self._started:
            await asyncio.to_thread(self._db._start_watcher)
            self._started = True

        while True:
            if self._buffer:
                return self._buffer.pop(0)
            events = await asyncio.to_thread(self._db._poll_events, 200)
            if events:
                self._buffer.extend(events)


class AsyncDirSQL:
    """Async wrapper around DirSQL.

    Usage:
        db = await AsyncDirSQL(root, tables=[...])
        results = await db.query("SELECT ...")
        async for event in db.watch():
            ...
    """

    def __init__(self, root, *, tables, ignore=None):
        self._root = root
        self._tables = tables
        self._ignore = ignore
        self._db = None

    def __await__(self):
        return self._init().__await__()

    async def _init(self):
        self._db = await asyncio.to_thread(
            DirSQL, self._root, tables=self._tables, ignore=self._ignore
        )
        return self

    async def query(self, sql):
        """Execute a SQL query asynchronously."""
        return await asyncio.to_thread(self._db.query, sql)

    def watch(self):
        """Start watching for file changes. Returns an async iterable of RowEvent."""
        return _WatchStream(self._db)
