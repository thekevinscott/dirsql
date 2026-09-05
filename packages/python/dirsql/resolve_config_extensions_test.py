"""Unit tests for `resolve_config_extension_specs`.

Every collaborator -- the config reader, the bare-name probe and the effectful
per-entry resolution -- is mocked, so these isolate the intervene-or-not
decision from the filesystem.
"""

from unittest import mock

import dirsql.resolve_config_extensions as mod


def _patch(*, loaded, bare, resolved="SPECS"):
    return (
        mock.patch.object(mod, "_load_extension_entries", return_value=loaded),
        mock.patch.object(mod, "_has_bare_name", return_value=bare),
        mock.patch.object(mod, "_resolve_entries", return_value=resolved),
    )


def describe_resolve_config_extension_specs():
    def it_returns_none_when_the_config_declares_no_extensions():
        load, bare, resolve = _patch(loaded=None, bare=True)
        with load as loader, bare, resolve as resolve_entries:
            assert mod.resolve_config_extension_specs("/cfg/.dirsql.toml") is None
        loader.assert_called_once_with("/cfg/.dirsql.toml")
        resolve_entries.assert_not_called()

    def it_returns_none_when_every_path_is_literal():
        load, bare, resolve = _patch(loaded=([{"path": "a.so"}], "/cfg"), bare=False)
        with load, bare as has_bare_name, resolve as resolve_entries:
            assert mod.resolve_config_extension_specs("/cfg/.dirsql.toml") is None
        has_bare_name.assert_called_once_with([{"path": "a.so"}])
        resolve_entries.assert_not_called()

    def it_resolves_every_entry_when_a_path_is_a_bare_package_name():
        entries = [{"path": "sqlite_vec"}, {"path": "ext/local.so"}]
        load, bare, resolve = _patch(loaded=(entries, "/cfg"), bare=True)
        with load, bare, resolve as resolve_entries:
            assert mod.resolve_config_extension_specs("/cfg/.dirsql.toml") == "SPECS"
        resolve_entries.assert_called_once_with(entries, "/cfg")
