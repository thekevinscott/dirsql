"""Unit tests for the DirSQL async wrapper."""

import os
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
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod,
                    "resolve_extension_path",
                    side_effect=lambda path, base, resolve_relative: f"R:{path}",
                ) as resolver,
            ):
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
                # Programmatic extension paths are resolved (bare names ->
                # installed package) before reaching the core (#298), against
                # the cwd and without making relative paths absolute
                # (programmatic semantics, unlike config-file entries).
                resolver.assert_called_once_with(
                    "ext/a.so", base=os.getcwd(), resolve_relative=False
                )
                assert db._db.extensions == [{"path": "R:ext/a.so", "entrypoint": None}]
                assert db._db.query_calls == ["SELECT 1"]
                assert results == [{"sql": "SELECT 1"}]

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
        @pytest.mark.asyncio
        async def it_appends_resolved_config_extensions_and_suppresses_the_core():
            # When the config resolver intervenes (a bare package name in the
            # config, #313), its resolved specs are appended after the
            # programmatic ones and the core's own config-extension loading is
            # suppressed.
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod,
                    "resolve_extension_path",
                    side_effect=lambda path, base, resolve_relative: f"R:{path}",
                ),
                patch.object(
                    async_mod,
                    "resolve_config_extension_specs",
                    return_value=[{"path": "/env/pkg/ext.so", "entrypoint": "init"}],
                ) as config_resolver,
            ):
                db = async_mod.DirSQL(
                    config="/cfg/.dirsql.toml",
                    extensions=[{"path": "ext/a.so"}],
                )
                await db.ready()

                config_resolver.assert_called_once_with("/cfg/.dirsql.toml")
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
                    "resolve_config_extension_specs",
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
            # `None` from the resolver (no bare package name in the config)
            # keeps the pre-#313 behavior: the core loads the config's own
            # extension entries.
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod, "resolve_config_extension_specs", return_value=None
                ) as config_resolver,
            ):
                db = async_mod.DirSQL(config="/cfg/.dirsql.toml")
                await db.ready()

                config_resolver.assert_called_once_with("/cfg/.dirsql.toml")
                assert db._db.extensions is None
                assert db._db.suppress_config_extensions is False

        @pytest.mark.asyncio
        async def it_never_consults_the_config_resolver_without_a_config():
            with (
                patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL),
                patch.object(
                    async_mod, "resolve_config_extension_specs"
                ) as config_resolver,
            ):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                config_resolver.assert_not_called()
                assert db._db.suppress_config_extensions is False

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
            stream = async_mod._WatchStream(_FakeWatcherDb(events=[[], ["event-a"]]))

            first = await stream.__anext__()

            assert first == "event-a"
            assert stream._db.poll_calls == [200, 200]

    def describe_construction():
        @pytest.mark.asyncio
        async def it_constructs_without_a_root_or_config():
            # The guard that raised TypeError on (None, None) is gone; the
            # wrapper forwards both to the core, which owns "no root"
            # validation (DirSQLBuilder::resolve). Construction must not raise.
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL()
                await db.ready()

                assert db._db.root is None
                assert db._db.config is None

    def describe_watch():
        @pytest.mark.asyncio
        async def it_returns_a_watch_stream_over_the_background_db():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                stream = db.watch()

                assert isinstance(stream, async_mod._WatchStream)
                assert stream._db is db._db
