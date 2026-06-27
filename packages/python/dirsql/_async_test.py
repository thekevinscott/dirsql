"""Unit tests for the DirSQL async wrapper."""

from unittest.mock import patch

import pytest

import dirsql._async as async_mod

# Literal copy of `dirsql.resolve_config.INTERPRET_ROOT_ENV`. Inlined rather
# than imported so this unit test stays isolated from that collaborator
# module (testing-conventions `unit lint`); the integration tests exercise
# the two sides agreeing on the name.
INTERPRET_ROOT_ENV = "DIRSQL_INTERPRET_ROOT"


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
    ):
        self.root = root
        self.tables = tables
        self.ignore = ignore
        self.config = config
        self.persist = persist
        self.persist_path = persist_path
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
                    "/tmp/root", tables=["table-a"], ignore=["**/*.tmp"]
                )
                await db.ready()

                results = await db.query("SELECT 1")

                assert db._db.root == "/tmp/root"
                assert db._db.tables == ["table-a"]
                assert db._db.ignore == ["**/*.tmp"]
                assert db._db.query_calls == ["SELECT 1"]
                assert results == [{"sql": "SELECT 1"}]

        @pytest.mark.asyncio
        async def it_propagates_initialization_errors():
            class _BoomDirSQL:
                def __init__(self, *args, **kwargs):
                    raise RuntimeError("boom")

            with patch.object(async_mod, "_RustDirSQL", _BoomDirSQL):
                db = async_mod.DirSQL("/tmp/root", tables=["table-a"])

                with pytest.raises(RuntimeError, match="boom"):
                    await db.ready()

    def describe_root_resolution():
        @pytest.mark.asyncio
        async def it_raises_when_no_root_no_config_and_no_interpret_env():
            # Normal SDK use: neither root, config, nor the interpret
            # launcher's env var -> the original TypeError must still fire.
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                with patch.object(async_mod.os, "environ", {}):
                    with pytest.raises(
                        TypeError,
                        match="requires either a root directory or a config",
                    ):
                        async_mod.DirSQL(tables=["table-a"])

        @pytest.mark.asyncio
        async def it_defaults_root_to_the_interpret_env_when_root_and_config_unset():
            # Inside `dirsql interpret`, DIRSQL_INTERPRET_ROOT carries the
            # config file's parent directory; a config with no explicit root
            # adopts it instead of raising (#251).
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                with patch.object(
                    async_mod.os, "environ", {INTERPRET_ROOT_ENV: "/cfg/parent"}
                ):
                    db = async_mod.DirSQL(tables=["table-a"])
                    await db.ready()
                    assert db._root == "/cfg/parent"
                    assert db._db.root == "/cfg/parent"

        @pytest.mark.asyncio
        async def it_prefers_an_explicit_root_over_the_interpret_env():
            # An explicit root always wins; the env var is a last resort.
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                with patch.object(
                    async_mod.os, "environ", {INTERPRET_ROOT_ENV: "/cfg/parent"}
                ):
                    db = async_mod.DirSQL("/explicit", tables=["table-a"])
                    await db.ready()
                    assert db._root == "/explicit"

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
