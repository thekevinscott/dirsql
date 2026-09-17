"""Unit tests for `_plan_entry_path`.

Both collaborators -- the shadow probe (`os.path.isfile`) and the package
locator -- are mocked, so these isolate the precedence rule from disk.
"""

import types
from unittest import mock

import dirsql.plan_entry_path as mod


def _literal(path):
    return types.SimpleNamespace(path=path, package=None, shadow=None)


def _package(name, shadow):
    return types.SimpleNamespace(path=None, package=name, shadow=shadow)


def describe_plan_entry_path():
    def it_passes_a_literal_entry_through():
        with mock.patch.object(mod, "_resolve_package") as resolve_package:
            assert mod._plan_entry_path(_literal("/cfg/ext/a.so")) == "/cfg/ext/a.so"
        resolve_package.assert_not_called()

    def it_prefers_a_shadowing_file_over_the_installed_package():
        with (
            mock.patch.object(mod.os.path, "isfile", return_value=True) as isfile,
            mock.patch.object(mod, "_resolve_package") as resolve_package,
        ):
            entry = _package("sqlite_vec", "/cfg/sqlite_vec")
            assert mod._plan_entry_path(entry) == "/cfg/sqlite_vec"
        isfile.assert_called_once_with("/cfg/sqlite_vec")
        resolve_package.assert_not_called()

    def it_locates_the_installed_package_when_nothing_shadows_it():
        with (
            mock.patch.object(mod.os.path, "isfile", return_value=False),
            mock.patch.object(
                mod, "_resolve_package", return_value="/site/sqlite_vec/vec0.so"
            ) as resolve_package,
        ):
            entry = _package("sqlite_vec", "/cfg/sqlite_vec")
            assert mod._plan_entry_path(entry) == "/site/sqlite_vec/vec0.so"
        resolve_package.assert_called_once_with("sqlite_vec")


def describe_module_wiring():
    def it_locates_packages_through_the_shared_resolver():
        assert mod._resolve_package.__module__ == "dirsql.resolve_package"
