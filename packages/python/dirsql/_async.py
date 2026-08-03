"""Async-by-default DirSQL wrapper."""

import asyncio
import os

from dirsql._dirsql import DirSQL as _RustDirSQL
from dirsql.resolve_config_extensions import resolve_configs_extension_specs
from dirsql.resolve_extension import resolve_extension_path


class _WatchStream:
    """Async iterator that polls for file events."""

    def __init__(self, owner):
        self._owner = owner
        self._db = None
        self._started = False
        self._buffer = []

    def __aiter__(self):
        return self

    async def __anext__(self):
        if not self._started:
            await self._owner.ready()
            db = self._owner._db
            assert db is not None  # ready() returned, so _init_bg set _db
            self._db = db
            await asyncio.to_thread(db._start_watcher)
            self._started = True

        db = self._db
        assert db is not None
        while True:
            if self._buffer:
                return self._buffer.pop(0)
            events = await asyncio.to_thread(db._poll_events, 200)
            if events:
                self._buffer.extend(events)


class DirSQL:
    """Async-by-default wrapper around the Rust DirSQL engine.

    Usage:
        # Programmatic:
        db = DirSQL(root, tables=[...])
        # From a config file:
        db = DirSQL(config="./my-config.toml")

        await db.ready()
        results = await db.query("SELECT ...")
        async for event in db.watch():
            ...

    The index root is the explicit ``root`` when given, else the process
    current working directory. A ``config`` file's location never sets the
    root -- it only supplies tables, ignore patterns, and extensions. There
    is no ``[dirsql].root`` config key. Constructing with neither ``root``
    nor ``config`` roots at the cwd (no error is raised).

    Path-table scans (``SELECT ... FROM './'``) respect ``.gitignore`` files
    by default. Pass ``no_ignore=True`` to restore the full walk; the built-in
    ``node_modules``/``.git`` defaults and any ``ignore`` patterns still
    apply.

    Pass ``persist=True`` to keep an on-disk SQLite cache (default location:
    ``<root>/.dirsql/cache.db``). Override the location with ``persist_path``.

    Pass ``extensions`` -- a list of ``{"path": ..., "entrypoint": ...}`` dicts
    (``entrypoint`` optional) -- to load SQLite extensions onto the connection
    at startup. Any ``[[dirsql.extension]]`` entries in a ``config`` file are
    appended after the programmatic ones. A ``path`` (programmatic or
    config-file) may be a bare **package name**, resolved from the installed
    package in the runtime env.
    """

    def __init__(
        self,
        root=None,
        *,
        tables=None,
        ignore=None,
        no_ignore=False,
        config=None,
        persist=False,
        persist_path=None,
        extensions=None,
    ):
        self._root = root
        self._tables = tables
        self._ignore = ignore
        self._no_ignore = no_ignore
        self._config = config
        # A single path or a list of paths; the list merges in order (each
        # config's [[table]] / ignore / [[dirsql.extension]] accumulate).
        self._config_paths = (
            []
            if config is None
            else [config]
            if isinstance(config, str)
            else list(config)
        )
        self._persist = persist
        self._persist_path = persist_path
        self._extensions = extensions
        self._db = None
        self._ready_event = asyncio.Event()
        self._init_error = None
        self._task = asyncio.ensure_future(self._init_bg())

    async def _init_bg(self):
        """Run the scan in the background."""
        try:
            self._db = await asyncio.to_thread(self._build_db)
        except Exception as exc:
            self._init_error = exc
        finally:
            self._ready_event.set()

    def _build_db(self):
        """Resolve extensions and construct the Rust-backed instance.

        Runs on a worker thread (via ``asyncio.to_thread``): both the
        package-name resolution and the core's initial scan are blocking.

        When the ``config`` file names an extension by bare package name, the
        SDK resolves every one of the config's ``[[dirsql.extension]]`` entries
        itself -- appended after the programmatic ones -- and suppresses the
        core's own config-extension loading so the entries are not loaded a
        second time (the core cannot resolve a bare name).
        """
        extensions = self._resolved_extensions()
        suppress = False
        if self._config_paths:
            config_extensions = resolve_configs_extension_specs(self._config_paths)
            if config_extensions is not None:
                extensions = [*(extensions or []), *config_extensions]
                suppress = True
        return _RustDirSQL(
            self._root,
            tables=self._tables,
            ignore=self._ignore,
            no_ignore=self._no_ignore,
            config=self._config_paths or None,
            persist=self._persist,
            persist_path=self._persist_path,
            extensions=extensions,
            suppress_config_extensions=suppress,
        )

    def _resolved_extensions(self):
        """Resolve each programmatic extension's ``path`` to a loadable file.

        A bare package name is resolved to the loadable installed in the
        runtime env; path-looking values pass through verbatim. Config-file
        ``[[dirsql.extension]]`` entries are handled by ``_build_db``.
        """
        if not self._extensions:
            return self._extensions
        return [
            {
                "path": resolve_extension_path(
                    e["path"], base=os.getcwd(), resolve_relative=False
                ),
                "entrypoint": e.get("entrypoint"),
            }
            for e in self._extensions
        ]

    async def ready(self):
        """Wait until the initial scan is complete.

        Raises any exception that occurred during init.
        Can be called multiple times safely.
        """
        await self._ready_event.wait()
        if self._init_error is not None:
            raise self._init_error

    async def query(self, sql):
        """Execute a SQL query asynchronously.

        Awaits :meth:`ready` first, so calling ``query`` before an explicit
        ``await db.ready()`` waits for the background scan (and re-raises any
        initialization error) instead of failing on a still-``None`` ``_db``.
        """
        await self.ready()
        db = self._db
        assert db is not None  # ready() returned, so _init_bg set _db
        return await asyncio.to_thread(db.query, sql)

    async def scan_failures(self):
        """The files the initial scan could not index.

        Each entry carries ``path`` (relative to the root) and ``message``
        (the hook's own error). Empty after a clean scan, which is the signal
        to check: a non-empty list means the index is *incomplete*, not wrong,
        and those files are retried on the next scan.

        Awaits :meth:`ready` first, so the scan has finished before the answer
        is read -- otherwise a caller could see an empty list simply because
        the scan had not reached the failing file yet.
        """
        await self.ready()
        db = self._db
        assert db is not None  # ready() returned, so _init_bg set _db
        return db.scan_failures()

    def watch(self):
        """Start watching for file changes. Returns an async iterable of RowEvent.

        Like :meth:`query`, the returned stream awaits :meth:`ready` on its
        first iteration before starting the watcher, so calling ``watch``
        before an explicit ``await db.ready()`` waits for the background scan
        (and surfaces any initialization error) instead of failing on a
        still-``None`` ``_db``.
        """
        return _WatchStream(self)
