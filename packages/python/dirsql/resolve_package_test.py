"""Unit tests for `_resolve_package`.

Every collaborator -- `importlib.util.find_spec`, the candidate walk
(`glob.glob`) and the core's `select_loadable` -- is mocked, so these isolate
the package-location logic from any installed package or disk.
"""

import types
from unittest import mock

import pytest

import dirsql.resolve_package as mod


def _spec(*, locations=None, origin=None):
    return types.SimpleNamespace(submodule_search_locations=locations, origin=origin)


def _resolving(*, spec=None, find_spec_error=None, glob=None, selected="/site/x/y.so"):
    find_spec = (
        mock.patch.object(mod.importlib.util, "find_spec", side_effect=find_spec_error)
        if find_spec_error
        else mock.patch.object(mod.importlib.util, "find_spec", return_value=spec)
    )
    return (
        find_spec,
        mock.patch.object(mod, "select_loadable", return_value=selected),
        mock.patch.object(mod._glob, "glob", **(glob or {"return_value": []})),
    )


def describe_locating_the_package_directory():
    def it_hands_the_core_every_file_under_the_package_dir():
        find_spec, selecting, globbing = _resolving(
            spec=_spec(locations=["/site/sqlite_vec"]),
            glob={
                "return_value": [
                    "/site/sqlite_vec/__init__.py",
                    "/site/sqlite_vec/vec0.so",
                ]
            },
            selected="/site/sqlite_vec/vec0.so",
        )
        with find_spec, selecting as select, globbing as glob:
            assert mod._resolve_package("sqlite_vec") == "/site/sqlite_vec/vec0.so"
        glob.assert_called_once_with("/site/sqlite_vec/**/*", recursive=True)
        select.assert_called_once_with(
            "sqlite_vec",
            ["/site/sqlite_vec"],
            ["/site/sqlite_vec/__init__.py", "/site/sqlite_vec/vec0.so"],
        )

    def it_falls_back_to_origin_dir_for_a_single_file_module():
        find_spec, selecting, globbing = _resolving(
            spec=_spec(origin="/site/sqlite_vec/__init__.py"),
            glob={"return_value": ["/site/sqlite_vec/vec0.so"]},
            selected="/site/sqlite_vec/vec0.so",
        )
        with find_spec, selecting as select, globbing:
            assert mod._resolve_package("sqlite_vec") == "/site/sqlite_vec/vec0.so"
        assert select.call_args.args[1] == ["/site/sqlite_vec"]

    def it_accumulates_candidates_across_every_package_dir():
        find_spec, selecting, globbing = _resolving(
            spec=_spec(locations=["/site/a", "/site/b"]),
            glob={"side_effect": [["/site/a/x.py"], ["/site/b/y.so"]]},
            selected="/site/b/y.so",
        )
        with find_spec, selecting as select, globbing:
            assert mod._resolve_package("x") == "/site/b/y.so"
        select.assert_called_once_with(
            "x", ["/site/a", "/site/b"], ["/site/a/x.py", "/site/b/y.so"]
        )


def describe_rejecting_an_unresolvable_package():
    def it_wraps_a_find_spec_import_error():
        find_spec, selecting, globbing = _resolving(find_spec_error=ImportError("boom"))
        with find_spec, selecting, globbing, pytest.raises(ValueError) as excinfo:
            mod._resolve_package("nope")
        assert "could not resolve extension package 'nope': boom" in str(excinfo.value)

    def it_wraps_a_find_spec_value_error():
        find_spec, selecting, globbing = _resolving(find_spec_error=ValueError("bad"))
        with find_spec, selecting, globbing, pytest.raises(ValueError) as excinfo:
            mod._resolve_package("nope")
        assert "could not resolve extension package 'nope': bad" in str(excinfo.value)

    def it_reports_a_package_that_is_not_installed():
        find_spec, selecting, globbing = _resolving(spec=None)
        with find_spec, selecting, globbing, pytest.raises(ValueError) as excinfo:
            mod._resolve_package("nope")
        assert "not installed" in str(excinfo.value)

    def it_reports_a_spec_with_no_package_directory():
        find_spec, selecting, globbing = _resolving(spec=_spec(origin="built-in"))
        with find_spec, selecting, globbing, pytest.raises(ValueError) as excinfo:
            mod._resolve_package("nope")
        assert "no package directory" in str(excinfo.value)

    def it_reports_a_spec_with_a_frozen_origin():
        find_spec, selecting, globbing = _resolving(spec=_spec(origin="frozen"))
        with find_spec, selecting, globbing, pytest.raises(ValueError) as excinfo:
            mod._resolve_package("nope")
        assert "no package directory" in str(excinfo.value)

    def it_reports_a_spec_with_no_origin_at_all():
        find_spec, selecting, globbing = _resolving(spec=_spec())
        with find_spec, selecting, globbing, pytest.raises(ValueError) as excinfo:
            mod._resolve_package("nope")
        assert "no package directory" in str(excinfo.value)


def describe_module_wiring():
    def it_selects_through_the_native_core():
        assert mod.select_loadable.__module__ in (None, "dirsql._dirsql")
