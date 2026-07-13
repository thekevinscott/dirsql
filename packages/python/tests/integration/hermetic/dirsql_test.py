"""Hermetic integration tests for the Python SDK.

These tests exercise the SDK public API with the Rust core
(``dirsql._dirsql.DirSQL``) mocked and no filesystem access. They
verify the SDK's glue -- offloading to threads, ready()/error
propagation, lazy watcher startup, event iteration, config-based
construction, and kwarg forwarding -- without touching the real
PyO3-backed engine.

Real-core behaviour (SQL semantics, scanning, diffing, watching) is
covered by ``tests/binding/`` (the SDK against the real core) and by
the Rust core's own suites.
"""

import asyncio
from unittest.mock import patch

import pytest

from dirsql import _async as async_mod


class _FakeRustDirSQL:
    """Test double for the PyO3 ``DirSQL`` class.

    Records constructor args and method calls so tests can assert the
    binding layer passes them through untouched.
    """

    instances: list = []

    def __init__(
        self,
        root=None,
        *,
        tables=None,
        ignore=None,
        config=None,
        persist=False,
        persist_path=None,
        extensions=None,
        suppress_config_extensions=False,
    ):
        self.root = root
        self.tables = tables
        self.ignore = ignore
        self.config = config
        self.persist = persist
        self.persist_path = persist_path
        self.extensions = extensions
        self.suppress_config_extensions = suppress_config_extensions
        self.queries: list[str] = []
        self.query_results: list = (
            [{"from_config": config}] if config is not None else [{"ok": 1}]
        )
        self.started = False
        self.poll_calls: list[int] = []
        # Scripted event batches; each poll returns the next batch.
        self.poll_batches: list[list] = []
        _FakeRustDirSQL.instances.append(self)

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
def mock_core():
    """Replace the Rust-backed ``_RustDirSQL`` alias in ``dirsql._async``."""
    with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
        yield _FakeRustDirSQL


@pytest.fixture
def to_thread_spy():
    """Wrap ``asyncio.to_thread`` to record the names of offloaded callables.

    Yields the recording list so a test can assert which work was pushed
    off the event loop.
    """
    calls: list[str] = []
    real_to_thread = asyncio.to_thread

    async def spy(func, *args, **kwargs):
        calls.append(getattr(func, "__name__", repr(func)))
        return await real_to_thread(func, *args, **kwargs)

    with patch.object(async_mod.asyncio, "to_thread", spy):
        yield calls


@pytest.fixture
def core_init_raises(request):
    """Patch the Rust core with a constructor that raises ``request.param``.

    Parametrize indirectly with the exception instance the core should
    raise on construction (init / config-load failure paths).
    """
    exc = request.param

    class Boom(_FakeRustDirSQL):
        def __init__(self, *a, **kw):
            raise exc

    with patch.object(async_mod, "_RustDirSQL", Boom):
        yield exc


def describe_binding_layer():
    def describe_async_offloading():
        # Feature: async-by-default API. See docs/reference/sdk.md and
        # packages/python/README.md ("DirSQL is async by default").
        @pytest.mark.asyncio
        async def it_offloads_init_via_to_thread(mock_core, to_thread_spy):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            assert "_build_db" in to_thread_spy, to_thread_spy

        @pytest.mark.asyncio
        async def it_offloads_query_via_to_thread(mock_core, to_thread_spy):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            await db.query("SELECT 1")
            assert "query" in to_thread_spy

    def describe_ready():
        # Feature: ready() awaits initial scan and surfaces init errors.
        # See docs/reference/sdk.md and packages/python/README.md.
        @pytest.mark.asyncio
        @pytest.mark.parametrize(
            "core_init_raises", [RuntimeError("init failed")], indirect=True
        )
        async def it_surfaces_init_exceptions(core_init_raises):
            db = async_mod.DirSQL("/root", tables=["t"])
            with pytest.raises(RuntimeError, match="init failed"):
                await db.ready()

        @pytest.mark.asyncio
        async def it_is_safe_to_call_repeatedly(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()
            await db.ready()
            await db.ready()
            assert len(_FakeRustDirSQL.instances) == 1

        @pytest.mark.asyncio
        @pytest.mark.parametrize(
            "core_init_raises", [ValueError("bad config")], indirect=True
        )
        async def it_re_raises_init_error_on_every_ready_call(core_init_raises):
            db = async_mod.DirSQL("/root", tables=["t"])
            with pytest.raises(ValueError):
                await db.ready()
            with pytest.raises(ValueError):
                await db.ready()

    def describe_query():
        # Feature: query() passes SQL to the engine. See
        # docs/reference/sdk.md and packages/python/README.md.
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
        # docs/reference/sdk.md and packages/python/README.md.
        @pytest.mark.asyncio
        async def it_lazily_starts_watcher_on_first_iteration(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()

            stream = db.watch()
            assert _FakeRustDirSQL.instances[0].started is False

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

    def describe_config_kwarg():
        # Feature: DirSQL(config=path) forwards to the Rust core. See
        # docs/reference/config.md and packages/python/README.md.
        @pytest.mark.asyncio
        async def it_forwards_config_path_to_core(mock_core):
            db = async_mod.DirSQL(config="/some/.dirsql.toml")
            await db.ready()

            inst = _FakeRustDirSQL.instances[-1]
            assert inst.config == ["/some/.dirsql.toml"]
            assert inst.root is None

            result = await db.query("SELECT 1")
            assert result == [{"from_config": ["/some/.dirsql.toml"]}]

        @pytest.mark.asyncio
        @pytest.mark.parametrize(
            "core_init_raises", [FileNotFoundError("/missing.toml")], indirect=True
        )
        async def it_surfaces_config_load_errors(core_init_raises):
            db = async_mod.DirSQL(config="/missing.toml")
            with pytest.raises(FileNotFoundError):
                await db.ready()

        @pytest.mark.asyncio
        async def it_forwards_construction_without_root_or_config_to_the_core(
            mock_core,
        ):
            # With neither root nor config the core roots at the cwd; the
            # wrapper forwards both as None.
            db = async_mod.DirSQL()
            await db.ready()

            inst = _FakeRustDirSQL.instances[-1]
            assert inst.root is None
            assert inst.config is None

    def describe_ignore_kwarg():
        # Feature: ignore patterns. See docs/howto/skip-files.md and
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

    def describe_persist_kwargs():
        # Feature: persist / persist_path. See docs/howto/persist.md.
        @pytest.mark.asyncio
        async def it_forwards_persist_kwargs_to_core(mock_core):
            db = async_mod.DirSQL(
                "/root",
                tables=["t"],
                persist=True,
                persist_path="/tmp/cache.db",
            )
            await db.ready()
            inst = _FakeRustDirSQL.instances[0]
            assert inst.persist is True
            assert inst.persist_path == "/tmp/cache.db"

        @pytest.mark.asyncio
        async def it_defaults_persist_to_false(mock_core):
            db = async_mod.DirSQL("/root", tables=["t"])
            await db.ready()
            inst = _FakeRustDirSQL.instances[0]
            assert inst.persist is False
            assert inst.persist_path is None
