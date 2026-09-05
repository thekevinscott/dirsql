"""Unit tests for `_resolve_entries`.

The one collaborator, the effectful `resolve_extension_path`, is mocked.
"""

from unittest import mock

import dirsql.resolve_entries as mod


def _patch():
    return mock.patch.object(
        mod,
        "resolve_extension_path",
        side_effect=lambda path, base, resolve_relative: f"R:{path}",
    )


def describe_resolve_entries():
    def it_resolves_nothing_for_an_empty_entry_list():
        with _patch() as resolver:
            assert mod._resolve_entries([], "/cfg") == []
            resolver.assert_not_called()

    def it_resolves_every_entry_against_the_base_directory():
        with _patch() as resolver:
            specs = mod._resolve_entries(
                [
                    {"path": "sqlite_vec", "entrypoint": "sqlite3_vec_init"},
                    {"path": "ext/local.so"},
                ],
                "/cfg",
            )
        assert specs == [
            {"path": "R:sqlite_vec", "entrypoint": "sqlite3_vec_init"},
            {"path": "R:ext/local.so", "entrypoint": None},
        ]
        resolver.assert_any_call("sqlite_vec", base="/cfg", resolve_relative=True)
        resolver.assert_any_call("ext/local.so", base="/cfg", resolve_relative=True)

    def it_normalizes_a_non_string_entrypoint_to_none():
        with _patch():
            specs = mod._resolve_entries(
                [{"path": "sqlite_vec", "entrypoint": 42}], "/cfg"
            )
        assert specs == [{"path": "R:sqlite_vec", "entrypoint": None}]
