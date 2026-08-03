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
        self.scan_failures_value = []

    def query(self, sql):
        self.query_calls.append(sql)
        return [{"sql": sql}]

    def scan_failures(self):
        return self.scan_failures_value


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


class _Failure:
    """Stand-in for the core's ScanFailure; the wrapper only passes it along."""

    def __init__(self, path, message):
        self.path = path
        self.message = message


class _ReadyOwner:
    """Fake DirSQL whose ``_db`` is already populated when ``ready`` returns."""

    def __init__(self, db):
        self._db = db

    async def ready(self):
        pass


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
                # Programmatic entries resolve against the cwd and keep
                # relative paths relative (unlike config-file entries).
                resolver.assert_called_once_with(
                    "ext/a.so", base=os.getcwd(), resolve_relative=False
                )
                assert db._db.extensions == [{"path": "R:ext/a.so", "entrypoint": None}]
                assert db._db.query_calls == ["SELECT 1"]
                assert results == [{"sql": "SELECT 1"}]

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
        async def it_awaits_ready_then_forwards_scan_failures():
            # Awaiting ready first is the point: reading before the scan
            # finishes would report an empty list for a scan that had simply
            # not reached the failing file yet.
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()
                db._db.scan_failures_value = [_Failure("bad.json", "boom")]

                (failure,) = await db.scan_failures()

                assert failure.path == "bad.json"
                assert failure.message == "boom"

        @pytest.mark.asyncio
        async def it_reports_no_scan_failures_for_a_clean_scan():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                assert await db.scan_failures() == []

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

                async def ready(self):
                    raise RuntimeError("boom")

            stream = async_mod._WatchStream(_BoomOwner())

            with pytest.raises(RuntimeError, match="boom"):
                await stream.__anext__()

    def describe_construction():
        @pytest.mark.asyncio
        async def it_constructs_without_a_root_or_config():
            # With neither root nor config the core roots at the cwd;
            # construction must not raise.
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL()
                await db.ready()

                assert db._db.root is None
                assert db._db.config is None

    def describe_watch():
        @pytest.mark.asyncio
        async def it_returns_a_watch_stream_bound_to_the_owner():
            with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
                db = async_mod.DirSQL("/tmp/root")
                await db.ready()

                stream = db.watch()

                assert isinstance(stream, async_mod._WatchStream)
                assert stream._owner is db
