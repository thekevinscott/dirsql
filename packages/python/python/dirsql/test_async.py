"""Unit tests for the async DirSQL wrapper."""

import pytest

from dirsql import _async as async_mod


class _FakeDirSQL:
    def __init__(self, root, *, tables, ignore=None):
        self.root = root
        self.tables = tables
        self.ignore = ignore
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


@pytest.mark.asyncio
async def test_ready_and_query_use_the_background_db(monkeypatch):
    monkeypatch.setattr(async_mod, "DirSQL", _FakeDirSQL)

    db = async_mod.AsyncDirSQL("/tmp/root", tables=["table-a"], ignore=["**/*.tmp"])
    await db.ready()

    results = await db.query("SELECT 1")

    assert db._db.root == "/tmp/root"
    assert db._db.tables == ["table-a"]
    assert db._db.ignore == ["**/*.tmp"]
    assert db._db.query_calls == ["SELECT 1"]
    assert results == [{"sql": "SELECT 1"}]


@pytest.mark.asyncio
async def test_ready_propagates_initialization_errors(monkeypatch):
    class _BoomDirSQL:
        def __init__(self, *args, **kwargs):
            raise RuntimeError("boom")

    monkeypatch.setattr(async_mod, "DirSQL", _BoomDirSQL)

    db = async_mod.AsyncDirSQL("/tmp/root", tables=["table-a"])

    with pytest.raises(RuntimeError, match="boom"):
        await db.ready()


@pytest.mark.asyncio
async def test_watch_stream_starts_and_buffers_events():
    stream = async_mod._WatchStream(_FakeWatcherDb(events=[["event-a", "event-b"]]))

    assert stream.__aiter__() is stream

    first = await stream.__anext__()
    second = await stream.__anext__()

    assert first == "event-a"
    assert second == "event-b"
    assert stream._db.started == 1
    assert stream._db.poll_calls == [200]
