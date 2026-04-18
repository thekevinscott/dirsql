"""Unit tests for the DirSQL async wrapper."""

from __future__ import annotations

import pytest

from dirsql import _async as async_mod


class _FakeRustDirSQL:
    def __init__(self, root=None, *, tables=None, ignore=None, config=None):
        self.root = root
        self.tables = tables
        self.ignore = ignore
        self.config = config
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
        async def it_uses_the_background_db(monkeypatch):
            monkeypatch.setattr(async_mod, "_RustDirSQL", _FakeRustDirSQL)

            db = async_mod.DirSQL("/tmp/root", tables=["table-a"], ignore=["**/*.tmp"])
            await db.ready()

            results = await db.query("SELECT 1")

            assert db._db.root == "/tmp/root"
            assert db._db.tables == ["table-a"]
            assert db._db.ignore == ["**/*.tmp"]
            assert db._db.query_calls == ["SELECT 1"]
            assert results == [{"sql": "SELECT 1"}]

        @pytest.mark.asyncio
        async def it_propagates_initialization_errors(monkeypatch):
            class _BoomDirSQL:
                def __init__(self, *args, **kwargs):
                    raise RuntimeError("boom")

            monkeypatch.setattr(async_mod, "_RustDirSQL", _BoomDirSQL)

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
