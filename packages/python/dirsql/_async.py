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
    """Merge construction kwargs with a `.dirsql.toml` into resolved state.

    Mirrors `DirSQLBuilder::resolve` in the Rust core: an explicit `root` wins
    over a config-supplied one; programmatic tables/ignore are followed by
    config-supplied ones; `persist=True` from either side enables persistence;
    an explicit `persist_path` overrides the config's. Path-valued config
    fields are resolved relative to the config file's parent.

    Returns a 5-tuple `(root, tables, ignore, persist, persist_path)` where
    `tables` is a list of `{"ddl", "glob", "strict"}` dicts (the canonical
    serialized shape) drawn from both the programmatic `Table` instances and
    the config's `[[table]]` entries.
    """
    cfg_root = None
    cfg_tables = []
    cfg_ignore = []
    cfg_persist = False
    cfg_persist_path = None

    if config is not None:
        with open(config, "rb") as f:
            raw = tomllib.load(f)
        cfg_parent = os.path.dirname(os.path.abspath(config)) or "."

        section = raw.get("dirsql", {}) or {}
        if "root" in section:
            r = section["root"]
            cfg_root = r if os.path.isabs(r) else os.path.join(cfg_parent, r)
        else:
            cfg_root = cfg_parent
        cfg_ignore = list(section.get("ignore", []) or [])
        cfg_persist = bool(section.get("persist", False))
        if "persist_path" in section:
            p = section["persist_path"]
            cfg_persist_path = p if os.path.isabs(p) else os.path.join(cfg_parent, p)

        for entry in raw.get("table", []) or []:
            if "ddl" not in entry:
                raise ValueError("Missing required field 'ddl' in [[table]] entry")
            if "glob" not in entry:
                raise ValueError("Missing required field 'glob' in [[table]] entry")
            cfg_tables.append(
                {
                    "ddl": entry["ddl"],
                    "glob": entry["glob"],
                    "strict": bool(entry.get("strict", False)),
                }
            )

    resolved_root = root if root is not None else cfg_root

    programmatic = [
        {"ddl": t.ddl, "glob": t.glob, "strict": bool(t.strict)}
        for t in (tables or [])
    ]
    resolved_tables = programmatic + cfg_tables

    resolved_ignore = list(ignore or []) + cfg_ignore
    resolved_persist = bool(persist) or cfg_persist
    resolved_persist_path = persist_path if persist_path is not None else cfg_persist_path

    return (
        resolved_root,
        resolved_tables,
        resolved_ignore,
        resolved_persist,
        resolved_persist_path,
    )


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
        root, tables, ignore, persist, persist_path = _resolve_config(
            self._root,
            self._tables,
            self._ignore,
            self._config,
            self._persist,
            self._persist_path,
        )
        return {
            "root": root,
            "tables": tables,
            "ignore": ignore,
            "persist": persist,
            "persist_path": persist_path,
        }
