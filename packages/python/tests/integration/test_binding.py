"""Integration tests for the Python SDK binding layer.

These tests exercise the async Python wrapper in ``dirsql._async`` in
isolation by mocking the Rust core (``dirsql._dirsql.DirSQL`` / its
``from_config`` classmethod). They verify the SDK's binding glue --
offloading to threads, ready()/error propagation, lazy watcher startup,
event iteration, config-based construction, and kwarg forwarding --
without touching the real PyO3-backed engine.

Core behaviour (SQL semantics, scanning, diffing, watching) is covered
by the Rust core's own unit tests and by the local-only e2e suite.
"""

import asyncio

import pytest

from dirsql import _async as async_mod


class _FakeRustDirSQL:
    """Test double for the PyO3 ``DirSQL`` class.

    Records constructor args and method calls so tests can assert the
    binding layer passes them through untouched.
    """

    instances: list = []

    def __init__(self, root, *, tables, ignore=None):
        self.root = root
        self.tables = tables
        self.ignore = ignore
        self.queries: list[str] = []
        self.query_results: list = [{"ok": 1}]
        self.started = False
        self.poll_calls: list[int] = []
        # Scripted event batches; each poll returns the next batch.
        self.poll_batches: list[list] = []
        _FakeRustDirSQL.instances.append(self)

    # Class-level from_config so we can swap it with a callable that
    # returns a fresh instance (mirrors the real classmethod shape).
    @classmethod
    def from_config(cls, path):
        inst = object.__new__(cls)
        inst.root = None
        inst.tables = None
        inst.ignore = None
        inst.queries = []
        inst.query_results = [{"from_config": path}]
        inst.started = False
        inst.poll_calls = []
        inst.poll_batches = []
        inst.config_path = path
        cls.instances.append(inst)
        return inst

    def query(self, sql):
        self.queries.append(sql)
        return self.query_results

    def _start_watcher(self):
        self.started = True

    def _poll_events(self, timeout_ms):
        self.poll_calls.append(timeout_ms)
        if self.poll_batches:
            return self.poll_batches.pop(0)
        return []


@pytest.fixture(autouse=True)
def _reset_instances():
    _FakeRustDirSQL.instances = []
    yield
    _FakeRustDirSQL.instances = []


@pytest.fixture
def mock_core(monkeypatch):
    """Replace the Rust-backed ``_RustDirSQL`` alias in ``dirsql._async``."""
    monkeypatch.setattr(async_mod, "_RustDirSQL", _FakeRustDirSQL)
    return _FakeRustDirSQL


def describe_binding_layer():
    def describe_async_offloading():
        # Feature: async-by-default API. See docs/guide/async.md and
        # packages/python/README.md ("DirSQL is async by default").
        @pytest.mark.asyncio
        async def it_offloads_init_via_to_thread(mock_core, monkeypatch):
            calls: list[str] = []
            real_to_thread = asyncio.to_thread

            async def spy(func, *args, **kwargs):
                calls.append(getattr(func, "__name__", repr(func)))
                return await real_to_thread(func, *args, **kwargs)

            monkeypatch.setattr(async_mod.asyncio, "to_thread", spy)

            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            assert any("DirSQL" in c or "FakeRustDirSQL" in c for c in calls), calls

        @pytest.mark.asyncio
        async def it_offloads_query_via_to_thread(mock_core, monkeypatch):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            calls: list[str] = []
            real_to_thread = asyncio.to_thread

            async def spy(func, *args, **kwargs):
                calls.append(getattr(func, "__name__", repr(func)))
                return await real_to_thread(func, *args, **kwargs)

            monkeypatch.setattr(async_mod.asyncio, "to_thread", spy)

            await db.query("SELECT 1")
            assert "query" in calls

    def describe_ready():
        # Feature: ready() awaits initial scan and surfaces init errors.
        # See docs/guide/async.md and packages/python/README.md.
        @pytest.mark.asyncio
        async def it_surfaces_init_exceptions(monkeypatch):
            class Boom(_FakeRustDirSQL):
                def __init__(self, *a, **kw):
                    raise RuntimeError("init failed")

            monkeypatch.setattr(async_mod, "_RustDirSQL", Boom)

            db = async_mod.DirSQL("/root", tables=["t"])
            with pytest.raises(RuntimeError, match="init failed"):
                await db.ready()

        @pytest.mark.asyncio
        async def it_is_safe_to_call_repeatedly(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()
            await db.ready()
            await db.ready()
            # Only one underlying instance should have been constructed.
            assert len(_FakeRustDirSQL.instances) == 1

        @pytest.mark.asyncio
        async def it_re_raises_init_error_on_every_ready_call(monkeypatch):
            class Boom(_FakeRustDirSQL):
                def __init__(self, *a, **kw):
                    raise ValueError("bad config")

            monkeypatch.setattr(async_mod, "_RustDirSQL", Boom)
            db = async_mod.DirSQL("/root", tables=["t"])
            with pytest.raises(ValueError):
                await db.ready()
            with pytest.raises(ValueError):
                await db.ready()

    def describe_query():
        # Feature: query() passes SQL to the engine. See
        # docs/guide/querying.md and packages/python/README.md.
        @pytest.mark.asyncio
        async def it_passes_sql_through_untouched(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            sql = "SELECT name, age FROM users WHERE age > 30 -- comment"
            result = await db.query(sql)

            assert _FakeRustDirSQL.instances[0].queries == [sql]
            assert result == [{"ok": 1}]

    def describe_watch():
        # Feature: watch() is an async iterator of RowEvent. See
        # docs/guide/watching.md and packages/python/README.md.
        @pytest.mark.asyncio
        async def it_lazily_starts_watcher_on_first_iteration(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            stream = db.watch()
            assert _FakeRustDirSQL.instances[0].started is False

            # Queue a single event so __anext__ returns.
            _FakeRustDirSQL.instances[0].poll_batches = [["evt-1"]]

            event = await stream.__anext__()
            assert event == "evt-1"
            assert _FakeRustDirSQL.instances[0].started is True

        @pytest.mark.asyncio
        async def it_drains_buffered_events_before_polling_again(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            fake = _FakeRustDirSQL.instances[0]
            fake.poll_batches = [["a", "b", "c"]]

            stream = db.watch()
            assert await stream.__anext__() == "a"
            assert await stream.__anext__() == "b"
            assert await stream.__anext__() == "c"
            # Only one poll happened; the rest came from the buffer.
            assert len(fake.poll_calls) == 1
            assert fake.poll_calls[0] == 200

        @pytest.mark.asyncio
        async def it_polls_until_events_arrive(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            fake = _FakeRustDirSQL.instances[0]
            fake.poll_batches = [[], [], ["late"]]

            stream = db.watch()
            event = await stream.__anext__()
            assert event == "late"
            assert len(fake.poll_calls) == 3

    def describe_from_config():
        # Feature: DirSQL.from_config(path) classmethod. See
        # docs/guide/config.md and packages/python/README.md.
        @pytest.mark.asyncio
        async def it_delegates_to_rust_from_config(mock_core):
            db = async_mod.DirSQL.from_config("/some/.dirsql.toml")
            await db.ready()

            inst = _FakeRustDirSQL.instances[-1]
            assert inst.config_path == "/some/.dirsql.toml"

            result = await db.query("SELECT 1")
            assert result == [{"from_config": "/some/.dirsql.toml"}]

        @pytest.mark.asyncio
        async def it_surfaces_config_load_errors(monkeypatch):
            class Boom(_FakeRustDirSQL):
                @classmethod
                def from_config(cls, path):
                    raise FileNotFoundError(path)

            monkeypatch.setattr(async_mod, "_RustDirSQL", Boom)
            db = async_mod.DirSQL.from_config("/missing.toml")
            with pytest.raises(FileNotFoundError):
                await db.ready()

    def describe_ignore_kwarg():
        # Feature: ignore patterns. See docs/guide/tables.md and
        # packages/python/README.md (ignore= kwarg on DirSQL).
        @pytest.mark.asyncio
        async def it_forwards_ignore_to_core(mock_core):
            ignore = ["**/node_modules/**", ".git"]
            db = async_mod.DirSQL("/root", tables=["t"], ignore=ignore)
            await db.ready()

            inst = _FakeRustDirSQL.instances[0]
            assert inst.root == "/root"
            assert inst.tables == ["t"]
            assert inst.ignore == ignore

        @pytest.mark.asyncio
        async def it_defaults_ignore_to_none(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()
            assert _FakeRustDirSQL.instances[0].ignore is None
