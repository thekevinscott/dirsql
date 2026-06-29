"""Unit tests for the DirSQL async wrapper."""

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
        config=None,
        persist=False,
        persist_path=None,
        extensions=None,
    ):
        self.root = root
        self.tables = tables
        self.ignore = ignore
        self.config = config
        self.persist = persist
        self.persist_path = persist_path
        self.extensions = extensions
        self.query_calls = []

    def query(self, sql):
        self.query_calls.append(sql)
        return [{"sql": sql}]


class _FakeWatcherDb:
    def __init__(self, events):
        self.events = list(events)
        self.started = 0
        self.poll_calls = []

    def _start_watcher(self):
        self.started += 1

    def _poll_events(self, timeout_ms):
        self.poll_calls.append(timeout_ms)
        if self.events:
            return self.events.pop(0)
        return []


def describe_DirSQL_async():
    def describe_ready_and_query():
        @pytest.mark.asyncio
        async def it_uses_the_background_db():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL(
                    "/tmp/root",
                    tables=["table-a"],
                    ignore=["**/*.tmp"],
                    extensions=[{"path": "ext/a.so"}],
                )
                await db.ready()

                results = await db.query("SELECT 1")

                assert db._db.root == "/tmp/root"
                assert db._db.tables == ["table-a"]
                assert db._db.ignore == ["**/*.tmp"]
                assert db._db.extensions == [{"path": "ext/a.so"}]
                assert db._db.query_calls == ["SELECT 1"]
                assert results == [{"sql": "SELECT 1"}]

        @pytest.mark.asyncio
        async def it_awaits_readiness_when_query_is_called_before_ready():
            # query() must await initialization itself: calling it without a
            # prior `await db.ready()` should still return rows, not raise
            # AttributeError from `self._db` still being None.
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root", tables=["table-a"])

                results = await db.query("SELECT 1")

                assert results == [{"sql": "SELECT 1"}]
                assert db._db.query_calls == ["SELECT 1"]

        @pytest.mark.asyncio
        async def it_propagates_initialization_errors_from_query():
            # An init failure must surface through query() too (it awaits
            # ready(), which re-raises), not as an AttributeError.
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
            stream = async_mod._WatchStream(
                _FakeWatcherDb(events=[["event-a", "event-b"]])
            )

            assert stream.__aiter__() is stream

            first = await stream.__anext__()
            second = await stream.__anext__()

            assert first == "event-a"
            assert second == "event-b"
            assert stream._db.started == 1
            assert stream._db.poll_calls == [200]

        @pytest.mark.asyncio
        async def it_polls_again_when_a_poll_returns_no_events():
            # An empty poll loops back for another poll rather than yielding;
            # the second poll's events are what surface.
            stream = async_mod._WatchStream(
                _FakeWatcherDb(events=[[], ["event-a"]])
            )

            first = await stream.__anext__()

            assert first == "event-a"
            assert stream._db.poll_calls == [200, 200]

    def describe_construction():
        def it_requires_either_a_root_or_a_config():
            with pytest.raises(
                TypeError, match="root directory or a config"
            ):
                async_mod.DirSQL()

    def describe_watch():
        @pytest.mark.asyncio
        async def it_returns_a_watch_stream_over_the_background_db():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                stream = db.watch()

                assert isinstance(stream, async_mod._WatchStream)
                assert stream._db is db._db

    def describe_to_dict():
        @pytest.mark.asyncio
        async def it_resolves_construction_state_via_resolve_config():
            sentinel = {"root": "/tmp/root"}
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod, "resolve_config", return_value=sentinel
                ) as resolve,
            ):
                db = async_mod.DirSQL(
                    "/tmp/root",
                    tables=["table-a"],
                    ignore=["**/*.tmp"],
                    persist=True,
                    persist_path="/tmp/cache.db",
                    extensions=[{"path": "ext/a.so"}],
                )
                await db.ready()

                assert db.__dict__ is sentinel
                resolve.assert_called_once_with(
                    "/tmp/root",
                    ["table-a"],
                    ["**/*.tmp"],
                    None,
                    True,
                    "/tmp/cache.db",
                    [{"path": "ext/a.so"}],
                )
