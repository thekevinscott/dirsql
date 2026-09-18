"""Unit tests for `resolve_extension_path`.

Both collaborators are mocked: the core's planner and the entry-to-path step
that owns the filesystem probe and the package lookup.
"""

from unittest import mock

import dirsql.resolve_extension as mod


def describe_resolve_extension_path():
    def it_plans_through_the_core_and_resolves_the_planned_entry():
        entry = object()
        with (
            mock.patch.object(mod, "plan_extension_path", return_value=entry) as plan,
            mock.patch.object(
                mod, "_plan_entry_path", return_value="/site/vec/vec0.so"
            ) as entry_path,
        ):
            out = mod.resolve_extension_path("vec", base="/cfg", resolve_relative=False)

        assert out == "/site/vec/vec0.so"
        plan.assert_called_once_with("vec", "/cfg", False)
        entry_path.assert_called_once_with(entry)

    def it_passes_resolve_relative_through_to_the_planner():
        with (
            mock.patch.object(mod, "plan_extension_path") as plan,
            mock.patch.object(mod, "_plan_entry_path"),
        ):
            mod.resolve_extension_path("ext/a.so", base="/cfg", resolve_relative=True)

        plan.assert_called_once_with("ext/a.so", "/cfg", True)
