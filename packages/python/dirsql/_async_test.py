"""Unit tests for the DirSQL async wrapper."""

import asyncio
import gc
import os
import threading
import weakref
from unittest.mock import patch

import pytest

import dirsql._async as async_mod


class _FakeRustDirSQL:
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
        suppress_config_extensions=False,
    ):
        self.root = root
        self.tables = tables
        self.ignore = ignore
        self.no_ignore = no_ignore
        self.config = config
        self.persist = persist
        self.persist_path = persist_path
        self.extensions = extensions
        self.suppress_config_extensions = suppress_config_extensions
        self.query_calls = []
        self.query_params = []
        self.watchers_started = 0
        self.watchers_stopped = 0

    def _start_watcher(self):
        self.watchers_started += 1

    def _stop_watcher(self):
        self.watchers_stopped += 1

    def query(self, sql, params=None):
        self.query_calls.append(sql)
        self.query_params.append(params)
        return [{"sql": sql}]


class _FakeWatcherDb:
    def __init__(self, events):
        self.events = list(events)
        self.started = 0
        self.stopped = 0
        self.poll_calls = []

    def _start_watcher(self):
        self.started += 1

    def _stop_watcher(self):
        self.stopped += 1

    def _poll_events(self, timeout_ms):
        self.poll_calls.append(timeout_ms)
        if self.events:
            return self.events.pop(0)
        return []


class _HeldPollDb(_FakeWatcherDb):
    """A watcher whose poll does not return until ``release`` is set."""

    def __init__(self, events):
        super().__init__(events)
        self.release = threading.Event()

    def _poll_events(self, timeout_ms):
        self.release.wait()
        return super()._poll_events(timeout_ms)


class _HeldStartDb(_FakeWatcherDb):
    """A watcher whose start does not return until ``release`` is set."""

    def __init__(self, events):
        super().__init__(events)
        self.entered = threading.Event()
        self.release = threading.Event()

    def _start_watcher(self):
        self.entered.set()
        self.release.wait()
        super()._start_watcher()


async def _until(condition):
    while not condition():
        await asyncio.sleep(0)


class _ReadyOwner:
    """Fake DirSQL whose ``_db`` is already populated when ``ready`` returns."""

    def __init__(self, db):
        self._db = db
        self._watch_streams = weakref.WeakSet()
        self._watch_poll = None

    async def ready(self):
        pass


def describe_DirSQL_async():
    def describe_ready_and_query():
        @pytest.mark.asyncio
        async def it_uses_the_background_db():
            with (
                patch.object(
                    async_mod,
                    "resolve_extension_path",
                    side_effect=lambda path, base, resolve_relative: f"R:{path}",
                ) as resolver,
            ):

                class InjectedDirSQL(async_mod.DirSQL):
                    def _new_core(self, root, **kwargs):
                        return _FakeRustDirSQL(root, **kwargs)

                db = InjectedDirSQL(
                    "/tmp/root",
                    tables=["table-a"],
                    ignore=["**/*.tmp"],
                    extensions=[{"path": "ext/a.so"}],
                )
                await db.ready()

                results = await db.query("SELECT 1")

                assert db._db.root == os.path.abspath("/tmp/root")
                assert db._db.tables == ["table-a"]
                assert db._db.ignore == ["**/*.tmp"]
                # Programmatic entries resolve against the cwd and keep
                # relative paths relative (unlike config-file entries).
                resolver.assert_called_once_with(
                    "ext/a.so", base=os.getcwd(), resolve_relative=False
                )
                assert db._db.extensions == [{"path": "R:ext/a.so", "entrypoint": None}]
                assert db._db.query_calls == ["SELECT 1"]
                assert results == [{"sql": "SELECT 1"}]
                assert db._db.query_params == [None]

        @pytest.mark.asyncio
        async def it_forwards_params_to_the_core():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                await db.query("SELECT ?", [1])

                assert db._db.query_params == [[1]]

        @pytest.mark.asyncio
        async def it_forwards_no_ignore_to_the_core():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root", no_ignore=True)
                await db.ready()

                assert db._db.no_ignore is True

        @pytest.mark.asyncio
        async def it_defaults_no_ignore_to_false():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                assert db._db.no_ignore is False

        @pytest.mark.asyncio
        async def it_passes_no_extensions_through_unresolved():
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(async_mod, "resolve_extension_path") as resolver,
            ):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()
                assert db._db.extensions is None
                resolver.assert_not_called()

    def describe_config_file_extensions():
        def it_imports_the_config_resolver_from_the_plural_module():
            assert (
                async_mod.resolve_configs_extension_specs.__module__
                == "dirsql.resolve_configs_extension_specs"
            )

        @pytest.mark.asyncio
        async def it_appends_resolved_config_extensions_and_suppresses_the_core():
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod,
                    "resolve_extension_path",
                    side_effect=lambda path, base, resolve_relative: f"R:{path}",
                ),
                patch.object(
                    async_mod,
                    "resolve_configs_extension_specs",
                    return_value=[{"path": "/env/pkg/ext.so", "entrypoint": "init"}],
                ) as config_resolver,
            ):
                db = async_mod.DirSQL(
                    config="/cfg/.dirsql.toml",
                    extensions=[{"path": "ext/a.so"}],
                )
                await db.ready()

                config_resolver.assert_called_once_with(["/cfg/.dirsql.toml"])
                assert db._db.extensions == [
                    {"path": "R:ext/a.so", "entrypoint": None},
                    {"path": "/env/pkg/ext.so", "entrypoint": "init"},
                ]
                assert db._db.suppress_config_extensions is True

        @pytest.mark.asyncio
        async def it_passes_config_extensions_alone_when_no_programmatic_ones():
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod,
                    "resolve_configs_extension_specs",
                    return_value=[{"path": "/env/pkg/ext.so", "entrypoint": None}],
                ),
            ):
                db = async_mod.DirSQL(config="/cfg/.dirsql.toml")
                await db.ready()

                assert db._db.extensions == [
                    {"path": "/env/pkg/ext.so", "entrypoint": None}
                ]
                assert db._db.suppress_config_extensions is True

        @pytest.mark.asyncio
        async def it_leaves_the_core_loading_when_the_resolver_declines():
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod, "resolve_configs_extension_specs", return_value=None
                ) as config_resolver,
            ):
                db = async_mod.DirSQL(config="/cfg/.dirsql.toml")
                await db.ready()

                config_resolver.assert_called_once_with(["/cfg/.dirsql.toml"])
                assert db._db.extensions is None
                assert db._db.suppress_config_extensions is False

        @pytest.mark.asyncio
        async def it_never_consults_the_config_resolver_without_a_config():
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod, "resolve_configs_extension_specs"
                ) as config_resolver,
            ):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                config_resolver.assert_not_called()
                assert db._db.suppress_config_extensions is False

        @pytest.mark.asyncio
        async def it_forwards_a_list_of_configs_and_resolves_across_them():
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod, "resolve_configs_extension_specs", return_value=None
                ) as config_resolver,
            ):
                db = async_mod.DirSQL(config=["/a.toml", "/b.toml"])
                await db.ready()

                config_resolver.assert_called_once_with(["/a.toml", "/b.toml"])
                assert db._db.config == ["/a.toml", "/b.toml"]

        @pytest.mark.asyncio
        async def it_awaits_readiness_when_query_is_called_before_ready():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root", tables=["table-a"])

                results = await db.query("SELECT 1")

                assert results == [{"sql": "SELECT 1"}]
                assert db._db.query_calls == ["SELECT 1"]

        @pytest.mark.asyncio
        async def it_propagates_initialization_errors_from_query():
            class _BoomDirSQL:
                def __init__(self, *args, **kwargs):
                    raise RuntimeError("boom")

            with patch.object(async_mod, "_RustDirSQL", _BoomDirSQL):
                db = async_mod.DirSQL("/tmp/root", tables=["table-a"])

                with pytest.raises(RuntimeError, match="boom"):
                    await db.query("SELECT 1")

        @pytest.mark.asyncio
        async def it_propagates_initialization_errors():
            class _BoomDirSQL:
                def __init__(self, *args, **kwargs):
                    raise RuntimeError("boom")

            with patch.object(async_mod, "_RustDirSQL", _BoomDirSQL):
                db = async_mod.DirSQL("/tmp/root", tables=["table-a"])

                with pytest.raises(RuntimeError, match="boom"):
                    await db.ready()

    def describe_watch_stream():
        @pytest.mark.asyncio
        async def it_starts_the_watcher_and_buffers_events():
            fake_db = _FakeWatcherDb(events=[["event-a", "event-b"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))

            assert stream.__aiter__() is stream

            first = await stream.__anext__()
            second = await stream.__anext__()

            assert first == "event-a"
            assert second == "event-b"
            assert stream._db is fake_db
            assert fake_db.started == 1
            assert fake_db.poll_calls == [200]

        @pytest.mark.asyncio
        async def it_polls_again_when_a_poll_returns_no_events():
            fake_db = _FakeWatcherDb(events=[[], ["event-a"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))

            first = await stream.__anext__()

            assert first == "event-a"
            assert fake_db.poll_calls == [200, 200]

        @pytest.mark.asyncio
        async def it_awaits_readiness_and_reads_the_db_at_iteration_start():
            # The owner's _db is None until ready() completes; the stream must
            # re-read it at first iteration rather than capturing None at
            # construction time.
            fake_db = _FakeWatcherDb(events=[["event-a"]])

            class _LateOwner:
                def __init__(self):
                    self._db = None
                    self._watch_streams = weakref.WeakSet()
                    self._watch_poll = None

                async def ready(self):
                    self._db = fake_db

            owner = _LateOwner()
            stream = async_mod._WatchStream(owner)

            first = await stream.__anext__()

            assert first == "event-a"
            assert stream._db is fake_db
            assert fake_db.started == 1

        @pytest.mark.asyncio
        async def it_surfaces_the_init_error_instead_of_attributeerror():
            class _BoomOwner:
                _db = None
                _watch_streams = weakref.WeakSet()
                _watch_poll = None

                async def ready(self):
                    raise RuntimeError("boom")

            stream = async_mod._WatchStream(_BoomOwner())

            with pytest.raises(RuntimeError, match="boom"):
                await stream.__anext__()

        @pytest.mark.asyncio
        async def it_gives_every_stream_every_event():
            fake_db = _FakeWatcherDb(events=[["event-a", "event-b"]])
            owner = _ReadyOwner(fake_db)
            first = async_mod._WatchStream(owner)
            second = async_mod._WatchStream(owner)

            got = [
                await first.__anext__(),
                await second.__anext__(),
                await second.__anext__(),
                await first.__anext__(),
            ]

            assert got == ["event-a", "event-a", "event-b", "event-b"]
            assert fake_db.poll_calls == [200]

        @pytest.mark.asyncio
        async def it_joins_a_poll_already_in_flight():
            release = threading.Event()

            class _SlowDb(_FakeWatcherDb):
                def _poll_events(self, timeout_ms):
                    release.wait()
                    return super()._poll_events(timeout_ms)

            fake_db = _SlowDb(events=[["event-a"]])
            owner = _ReadyOwner(fake_db)
            first = async_mod._WatchStream(owner)
            second = async_mod._WatchStream(owner)

            leading = asyncio.ensure_future(first.__anext__())
            while owner._watch_poll is None:
                await asyncio.sleep(0)
            joining = asyncio.ensure_future(second.__anext__())
            while not second._started:
                await asyncio.sleep(0)
            release.set()

            assert await asyncio.gather(leading, joining) == ["event-a", "event-a"]
            assert fake_db.poll_calls == [200]

        @pytest.mark.asyncio
        async def it_keeps_the_batch_when_the_polling_consumer_is_cancelled():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            owner = _ReadyOwner(fake_db)
            first = async_mod._WatchStream(owner)
            second = async_mod._WatchStream(owner)

            polling = asyncio.ensure_future(first.__anext__())
            while owner._watch_poll is None:
                await asyncio.sleep(0)
            polling.cancel()

            assert await second.__anext__() == "event-a"
            assert fake_db.poll_calls == [200]

        @pytest.mark.asyncio
        async def it_stops_feeding_a_dropped_stream():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            owner = _ReadyOwner(fake_db)
            kept = async_mod._WatchStream(owner)
            async_mod._WatchStream(owner)

            assert await kept.__anext__() == "event-a"
            assert list(owner._watch_streams) == [kept]

        @pytest.mark.asyncio
        async def it_raises_the_error_of_a_failed_poll():
            class _BoomDb(_FakeWatcherDb):
                def _poll_events(self, timeout_ms):
                    raise RuntimeError("poll failed")

            stream = async_mod._WatchStream(_ReadyOwner(_BoomDb(events=[])))

            with pytest.raises(RuntimeError, match="poll failed"):
                await stream.__anext__()

    def describe_dropped_stream():
        @pytest.mark.asyncio
        async def it_stops_the_core_when_the_last_stream_is_dropped():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))
            await stream.__anext__()

            del stream
            gc.collect()

            assert fake_db.stopped == 1

        @pytest.mark.asyncio
        async def it_keeps_the_core_attached_while_another_stream_lives():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            owner = _ReadyOwner(fake_db)
            kept = async_mod._WatchStream(owner)
            dropped = async_mod._WatchStream(owner)
            await dropped.__anext__()

            del dropped
            gc.collect()

            assert fake_db.stopped == 0
            assert await kept.__anext__() == "event-a"

        @pytest.mark.asyncio
        async def it_leaves_the_stop_to_a_poll_in_flight():
            fake_db = _HeldPollDb(events=[["event-a"]])
            owner = _ReadyOwner(fake_db)
            stream = async_mod._WatchStream(owner)
            waiting = asyncio.ensure_future(stream.__anext__())
            await _until(lambda: owner._watch_poll is not None)
            waiting.cancel()
            with pytest.raises(asyncio.CancelledError):
                await waiting

            del stream, waiting
            gc.collect()
            stopped_under_the_poll = fake_db.stopped
            fake_db.release.set()
            await owner._watch_poll

            assert stopped_under_the_poll == 0
            assert fake_db.stopped == 1

        @pytest.mark.asyncio
        async def it_has_nothing_to_stop_before_the_core_is_built():
            class _UnbuiltOwner:
                _db = None
                _watch_poll = None

                def __init__(self):
                    self._watch_streams = weakref.WeakSet()

            owner = _UnbuiltOwner()
            stream = async_mod._WatchStream(owner)

            del stream
            gc.collect()

            assert list(owner._watch_streams) == []

    def describe_aclose():
        @pytest.mark.asyncio
        async def it_ends_iteration():
            fake_db = _FakeWatcherDb(events=[["event-a", "event-b"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))
            assert await stream.__anext__() == "event-a"

            await stream.aclose()

            with pytest.raises(StopAsyncIteration):
                await stream.__anext__()
            assert fake_db.poll_calls == [200]

        @pytest.mark.asyncio
        async def it_releases_the_buffer():
            fake_db = _FakeWatcherDb(events=[["event-a", "event-b"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))
            await stream.__anext__()
            assert stream._buffer == ["event-b"]

            await stream.aclose()

            assert stream._buffer == []

        @pytest.mark.asyncio
        async def it_stops_the_core_when_the_last_stream_closes():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            owner = _ReadyOwner(fake_db)
            first = async_mod._WatchStream(owner)
            second = async_mod._WatchStream(owner)
            await first.__anext__()

            await first.aclose()
            assert fake_db.stopped == 0
            assert list(owner._watch_streams) == [second]

            await second.aclose()
            assert fake_db.stopped == 1

        @pytest.mark.asyncio
        async def it_stops_feeding_a_closed_stream():
            fake_db = _FakeWatcherDb(events=[["event-a"], ["event-b"]])
            owner = _ReadyOwner(fake_db)
            closed = async_mod._WatchStream(owner)
            kept = async_mod._WatchStream(owner)
            await closed.__anext__()
            await closed.aclose()
            await kept.__anext__()

            assert await kept.__anext__() == "event-b"
            assert closed._buffer == []

        @pytest.mark.asyncio
        async def it_can_be_closed_twice():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))
            await stream.__anext__()

            await stream.aclose()
            await stream.aclose()

            with pytest.raises(StopAsyncIteration):
                await stream.__anext__()
            assert fake_db.stopped == 1

        @pytest.mark.asyncio
        async def it_leaves_nothing_to_stop_when_a_closed_stream_is_dropped():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))
            await stream.aclose()

            del stream
            gc.collect()

            assert fake_db.stopped == 1

        @pytest.mark.asyncio
        async def it_never_starts_the_watcher_for_a_stream_closed_unread():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))

            await stream.aclose()

            with pytest.raises(StopAsyncIteration):
                await stream.__anext__()
            assert fake_db.started == 0
            assert fake_db.poll_calls == []

        @pytest.mark.asyncio
        async def it_wakes_a_consumer_waiting_in_another_task():
            fake_db = _HeldPollDb(events=[["event-a"]])
            owner = _ReadyOwner(fake_db)
            stream = async_mod._WatchStream(owner)
            waiting = asyncio.ensure_future(stream.__anext__())

            try:
                await _until(lambda: stream._waiter is not None)
                await stream.aclose()
                with pytest.raises(StopAsyncIteration):
                    await waiting
                stopped_under_the_poll = fake_db.stopped
            finally:
                fake_db.release.set()
            await owner._watch_poll

            assert stopped_under_the_poll == 0
            assert fake_db.stopped == 1

        @pytest.mark.asyncio
        async def it_stops_the_core_again_when_closed_while_the_watcher_starts():
            fake_db = _HeldStartDb(events=[["event-a"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))
            starting = asyncio.ensure_future(stream.__anext__())
            await _until(fake_db.entered.is_set)

            try:
                await stream.aclose()
                stopped_on_close = fake_db.stopped
            finally:
                fake_db.release.set()

            with pytest.raises(StopAsyncIteration):
                await starting
            assert stopped_on_close == 1
            assert fake_db.stopped == 2
            assert fake_db.poll_calls == []

        @pytest.mark.asyncio
        async def it_closes_the_stream_on_leaving_async_with():
            fake_db = _FakeWatcherDb(events=[["event-a"]])
            stream = async_mod._WatchStream(_ReadyOwner(fake_db))

            async with stream as events:
                assert events is stream
                assert await events.__anext__() == "event-a"

            with pytest.raises(StopAsyncIteration):
                await stream.__anext__()
            assert fake_db.stopped == 1

        @pytest.mark.asyncio
        async def it_lets_an_error_leave_async_with():
            stream = async_mod._WatchStream(_ReadyOwner(_FakeWatcherDb(events=[])))

            with pytest.raises(RuntimeError, match="consumer failed"):
                async with stream:
                    raise RuntimeError("consumer failed")

    def describe_construction():
        @pytest.mark.asyncio
        async def it_roots_at_the_cwd_without_a_root_or_config():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL()
                await db.ready()

                assert db._db.root == os.getcwd()
                assert db._db.config is None

        @pytest.mark.asyncio
        async def it_resolves_a_relative_root_against_the_cwd_at_construction():
            built = os.path.abspath("/built")
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                with patch.object(async_mod.os, "getcwd", return_value=built):
                    db = async_mod.DirSQL("tree")
                await db.ready()

            assert db._db.root == os.path.join(built, "tree")

        @pytest.mark.asyncio
        async def it_leaves_an_unreadable_cwd_for_the_core_to_report():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                with patch.object(
                    async_mod.os, "getcwd", side_effect=FileNotFoundError
                ):
                    db = async_mod.DirSQL()
                await db.ready()

            assert db._db.root is None

    def describe_docstring():
        def it_names_no_cache_path_inside_the_root():
            assert ".dirsql/cache.db" not in async_mod.DirSQL.__doc__

    def describe_watch():
        @pytest.mark.asyncio
        async def it_returns_a_watch_stream_bound_to_the_owner():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                stream = db.watch()

                assert isinstance(stream, async_mod._WatchStream)
                assert stream._owner is db

        @pytest.mark.asyncio
        async def it_starts_the_watcher_before_returning_once_ready():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                db.watch()

                assert db._db.watchers_started == 1

        @pytest.mark.asyncio
        async def it_starts_the_watcher_before_ready_resolves_when_called_early():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")

                stream = db.watch()
                await db.ready()

                assert stream is not None
                assert db._db.watchers_started == 1

        @pytest.mark.asyncio
        async def it_leaves_the_watcher_stopped_without_a_stream():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                assert db._db.watchers_started == 0

        @pytest.mark.asyncio
        async def it_starts_the_watcher_again_after_the_last_stream_closed():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()
                await db.watch().aclose()
                assert db._db.watchers_stopped == 1

                db.watch()

                assert db._db.watchers_started == 2

        @pytest.mark.asyncio
        async def it_stops_the_core_when_the_early_stream_goes_as_the_watcher_starts():
            entered = threading.Event()
            release = threading.Event()

            class _HeldStartDirSQL(_FakeRustDirSQL):
                def _start_watcher(self):
                    entered.set()
                    release.wait()
                    super()._start_watcher()

            with patch.object(async_mod, "_RustDirSQL", _HeldStartDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                stream = db.watch()
                await _until(entered.is_set)

                try:
                    await stream.aclose()
                finally:
                    release.set()
                await db.ready()

                assert db._db.watchers_started == 1
                assert db._db.watchers_stopped == 2

        @pytest.mark.asyncio
        async def it_leaves_an_early_start_failure_to_the_stream():
            class _NoWatchDirSQL(_FakeRustDirSQL):
                def _start_watcher(self):
                    raise RuntimeError("no named tables")

            with patch.object(async_mod, "_RustDirSQL", _NoWatchDirSQL):
                db = async_mod.DirSQL("/tmp/root")

                stream = db.watch()
                await db.ready()

                with pytest.raises(RuntimeError, match="no named tables"):
                    await stream.__anext__()


class _FakeLockDb:
    def __init__(self):
        self.calls = []

    def _lock(self):
        self.calls.append("lock")

    def _unlock(self):
        self.calls.append("unlock")


def describe_lock():
    def it_holds_the_core_lock_for_the_block():
        db = _FakeLockDb()
        owner = _ReadyOwner(db)
        with async_mod._IndexLock(owner):
            assert db.calls == ["lock"]
        assert db.calls == ["lock", "unlock"]

    def it_releases_the_core_lock_when_the_block_raises():
        db = _FakeLockDb()
        with (
            pytest.raises(ValueError, match="boom"),
            async_mod._IndexLock(_ReadyOwner(db)),
        ):
            raise ValueError("boom")
        assert db.calls == ["lock", "unlock"]

    def it_refuses_before_the_index_is_built():
        owner = _ReadyOwner(None)
        with (
            pytest.raises(RuntimeError, match=r"await db\.ready\(\) first"),
            async_mod._IndexLock(owner),
        ):
            pass

    @pytest.mark.asyncio
    async def it_is_a_property_of_DirSQL_over_its_core():
        db_ = _FakeLockDb()

        class InjectedDirSQL(async_mod.DirSQL):
            def _new_core(self, root, **kwargs):
                return db_

        db = InjectedDirSQL("/tmp/root")
        await db.ready()
        with db.lock as held:
            assert isinstance(held, async_mod._IndexLock)
        assert db_.calls == ["lock", "unlock"]
