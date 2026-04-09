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
        db = AsyncDirSQL(root, tables=[...])
        await db.ready()
        results = await db.query("SELECT ...")
        async for event in db.watch():
            ...
    """

    def __init__(self, root, *, tables, ignore=None):
        self._root = root
        self._tables = tables
        self._ignore = ignore
        self._db = None
        self._ready_event = asyncio.Event()
        self._init_error = None
        self._task = asyncio.ensure_future(self._init_bg())

    @classmethod
    def from_config(cls, path):
        """Create an AsyncDirSQL from a .dirsql.toml config file.

        Returns an AsyncDirSQL instance. Call ``await db.ready()`` before querying.
        """
        instance = object.__new__(cls)
        instance._root = None
        instance._tables = None
        instance._ignore = None
        instance._db = None
        instance._ready_event = asyncio.Event()
        instance._init_error = None
        instance._task = asyncio.ensure_future(instance._init_from_config(path))
        return instance

    async def _init_from_config(self, path):
        """Run from_config scan in the background."""
        try:
            self._db = await asyncio.to_thread(DirSQL.from_config, path)
        except Exception as exc:
            self._init_error = exc
        finally:
            self._ready_event.set()

    async def _init_bg(self):
        """Run the scan in the background."""
        try:
            self._db = await asyncio.to_thread(
                DirSQL, self._root, tables=self._tables, ignore=self._ignore
            )
        except Exception as exc:
            self._init_error = exc
        finally:
            self._ready_event.set()

    async def ready(self):
        """Wait until the initial scan is complete.

        Raises any exception that occurred during init.
        Can be called multiple times safely.
        """
        await self._ready_event.wait()
        if self._init_error is not None:
            raise self._init_error

    async def query(self, sql):
        """Execute a SQL query asynchronously."""
        return await asyncio.to_thread(self._db.query, sql)

    def watch(self):
        """Start watching for file changes. Returns an async iterable of RowEvent."""
        return _WatchStream(self._db)
