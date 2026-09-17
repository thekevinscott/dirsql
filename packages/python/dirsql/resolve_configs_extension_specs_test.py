"""Unit tests for `resolve_configs_extension_specs`.

Every collaborator is mocked -- the core's planner, the config read, and the
per-entry path resolution -- so what is left under test is the marshaling:
absolute paths and contents in, resolved specs out.
"""

import types
from unittest import mock

import dirsql.resolve_configs_extension_specs as mod


def _entry(entrypoint=None):
    return types.SimpleNamespace(entrypoint=entrypoint)


def _resolving(plan, *, paths=None):
    return (
        mock.patch.object(mod, "plan_config_extensions", return_value=plan),
        mock.patch.object(mod, "_read_config", side_effect=lambda p: f"BODY{p}"),
        mock.patch.object(
            mod, "_plan_entry_path", side_effect=list(paths or []) or None
        ),
    )


def describe_resolve_configs_extension_specs():
    def it_hands_the_core_each_absolute_path_with_its_contents():
        planning, reading, resolving = _resolving(None)
        with (
            mock.patch.object(mod.os.path, "abspath", side_effect=lambda p: "/abs" + p),
            planning as plan,
            reading,
            resolving,
        ):
            assert mod.resolve_configs_extension_specs(["/a.toml", "/b.toml"]) is None
        plan.assert_called_once_with(
            [("/abs/a.toml", "BODY/a.toml"), ("/abs/b.toml", "BODY/b.toml")]
        )

    def it_returns_none_when_the_core_declines_to_intervene():
        planning, reading, resolving = _resolving(None)
        with planning, reading, resolving as entry_path:
            assert mod.resolve_configs_extension_specs(["/cfg/a.toml"]) is None
        entry_path.assert_not_called()

    def it_returns_none_for_an_empty_list():
        planning, reading, resolving = _resolving(None)
        with planning as plan, reading, resolving:
            assert mod.resolve_configs_extension_specs([]) is None
        plan.assert_called_once_with([])

    def it_pairs_each_resolved_path_with_its_entrypoint():
        planning, reading, resolving = _resolving(
            [_entry("sqlite3_a_init"), _entry()], paths=["/cfg/a.so", "/site/vec0.so"]
        )
        with planning, reading, resolving as entry_path:
            specs = mod.resolve_configs_extension_specs(["/cfg/a.toml"])
        assert specs == [
            {"path": "/cfg/a.so", "entrypoint": "sqlite3_a_init"},
            {"path": "/site/vec0.so", "entrypoint": None},
        ]
        assert entry_path.call_count == 2


def describe_module_wiring():
    # These pin where this module binds its collaborators from, so a
    # mis-pointed import is a failure rather than a silent re-export.
    def it_reads_each_config_through_the_shared_reader():
        assert mod._read_config.__module__ == "dirsql.read_config"

    def it_resolves_each_planned_entry_through_the_shared_helper():
        assert mod._plan_entry_path.__module__ == "dirsql.plan_entry_path"
