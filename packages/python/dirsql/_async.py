"""Async-by-default DirSQL wrapper."""

import asyncio
import os
import tomllib

from dirsql._dirsql import DirSQL as _RustDirSQL


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


def _resolve_config(root, tables, ignore, config, persist, persist_path):
    """Merge kwargs with a `.dirsql.toml` into the serialized state shape.

    Mirrors `DirSQLBuilder::resolve` in the Rust core: explicit kwargs win
    for scalars; tables and ignore lists are concatenated; persist is OR-ed;
    path-valued config fields resolve relative to the config file's parent.
    """
    cfg, cfg_tables, cfg_dir = {}, [], None
    if config is not None:
        with open(config, "rb") as f:
            doc = tomllib.load(f)
        cfg = doc.get("dirsql") or {}
        cfg_tables = doc.get("table") or []
        cfg_dir = os.path.dirname(os.path.abspath(config))

    def _abs(p):
        return p if os.path.isabs(p) else os.path.join(cfg_dir, p)

    return {
        "root": root or (_abs(cfg["root"]) if "root" in cfg else cfg_dir),
        "tables": [
            {"ddl": t.ddl, "glob": t.glob, "strict": bool(t.strict)}
            for t in (tables or [])
        ]
        + [
            {"ddl": e.get("ddl"), "glob": e.get("glob"), "strict": bool(e.get("strict"))}
            for e in cfg_tables
        ],
        "ignore": list(ignore or []) + list(cfg.get("ignore") or []),
        "persist": bool(persist or cfg.get("persist")),
        "persist_path": persist_path
        or (_abs(cfg["persist_path"]) if "persist_path" in cfg else None),
    }


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

    At least one of ``root`` or ``config`` must be supplied. When both are
    set, the explicit ``root`` wins over any ``[dirsql].root`` in the config
    file (a warning is emitted on stderr).

    Pass ``persist=True`` to keep an on-disk SQLite cache (default location:
    ``<root>/.dirsql/cache.db``). Override the location with ``persist_path``.
    """

    def __init__(
        self,
        root=None,
        *,
        tables=None,
        ignore=None,
        config=None,
        persist=False,
        persist_path=None,
    ):
        if root is None and config is None:
            raise TypeError("DirSQL requires either a root directory or a config= path")
        self._root = root
        self._tables = tables
        self._ignore = ignore
        self._config = config
        self._persist = persist
        self._persist_path = persist_path
        self._db = None
        self._ready_event = asyncio.Event()
        self._init_error = None
        self._task = asyncio.ensure_future(self._init_bg())

    async def _init_bg(self):
        """Run the scan in the background."""
        try:
            self._db = await asyncio.to_thread(
                _RustDirSQL,
                self._root,
                tables=self._tables,
                ignore=self._ignore,
                config=self._config,
                persist=self._persist,
                persist_path=self._persist_path,
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

    @property
    def __dict__(self):
        """Resolved runtime state as a JSON-serializable dict.

        ``vars(db)`` and ``json.dumps(vars(db))`` both work and return the
        same shape as ``DirSQLConfig`` on the Rust side and ``toJSON()`` in
        the TypeScript SDK (modulo ``persist_path`` ↔ ``persistPath``).

        Resolution -- including reading the ``.dirsql.toml`` if ``config=``
        was supplied -- runs on each access. Available immediately after
        construction; no need to ``await db.ready()`` first. Excludes
        ``config`` (already absorbed into ``root`` / ``tables`` / ``ignore``),
        per-table ``extract`` (closures aren't serializable), and per-table
        ``name`` (derivable from DDL).
        """
        return _resolve_config(
            self._root,
            self._tables,
            self._ignore,
            self._config,
            self._persist,
            self._persist_path,
        )
