"""Unit tests for `_resolve_package`.

Every collaborator -- `importlib.util.find_spec`, the loadable glob
(`glob.glob`) and the platform pattern table -- is mocked, so these isolate the
package-location logic from any installed package or disk.
"""

import types
from unittest import mock

import pytest

import dirsql.resolve_package as mod


def _spec(*, locations=None, origin=None):
    return types.SimpleNamespace(submodule_search_locations=locations, origin=origin)


def _resolving(*, spec=None, find_spec_error=None, patterns=("*.so",), glob=None):
    find_spec = (
        mock.patch.object(mod.importlib.util, "find_spec", side_effect=find_spec_error)
        if find_spec_error
        else mock.patch.object(mod.importlib.util, "find_spec", return_value=spec)
    )
    return (
        find_spec,
        mock.patch.object(mod, "_platform_patterns", return_value=patterns),
        mock.patch.object(mod._glob, "glob", **(glob or {})),
    )


def describe_locating_the_package_directory():
    def it_globs_the_platform_loadable_inside_the_package_dir():
        find_spec, patterns, globbing = _resolving(
            spec=_spec(locations=["/site/sqlite_vec"]),
            glob={"return_value": ["/site/sqlite_vec/vec0.so"]},
        )
        with find_spec, patterns, globbing as glob:
            assert mod._resolve_package("sqlite_vec") == "/site/sqlite_vec/vec0.so"
        glob.assert_called_once_with("/site/sqlite_vec/**/*.so", recursive=True)

    def it_falls_back_to_origin_dir_for_a_single_file_module():
        find_spec, patterns, globbing = _resolving(
            spec=_spec(origin="/site/sqlite_vec/__init__.py"),
            glob={"return_value": ["/site/sqlite_vec/vec0.so"]},
        )
        with find_spec, patterns, globbing as glob:
            assert mod._resolve_package("sqlite_vec") == "/site/sqlite_vec/vec0.so"
        glob.assert_called_once_with("/site/sqlite_vec/**/*.so", recursive=True)

    def it_globs_every_pattern_the_platform_declares():
        find_spec, patterns, globbing = _resolving(
            spec=_spec(locations=["/site/x"]),
            patterns=("*.dll", "*.pyd"),
            glob={"side_effect": [["/site/x/y.dll"], []]},
        )
        with find_spec, patterns, globbing as glob:
            assert mod._resolve_package("x") == "/site/x/y.dll"
        assert [c.args[0] for c in glob.call_args_list] == [
            "/site/x/**/*.dll",
            "/site/x/**/*.pyd",
        ]

    def it_globs_every_declared_package_directory():
        find_spec, patterns, globbing = _resolving(
            spec=_spec(locations=["/site/a", "/site/b"]),
            glob={"side_effect": [[], ["/site/b/y.so"]]},
        )
        with find_spec, patterns, globbing as glob:
            assert mod._resolve_package("x") == "/site/b/y.so"
        assert [c.args[0] for c in glob.call_args_list] == [
            "/site/a/**/*.so",
            "/site/b/**/*.so",
        ]


def describe_unresolvable_packages():
    def it_errors_when_the_package_is_not_installed():
        find_spec, patterns, globbing = _resolving(spec=None)
        with (
            find_spec,
            patterns,
            globbing,
            pytest.raises(
                ValueError,
                match=r"could not resolve extension package 'nope': not installed",
            ),
        ):
            mod._resolve_package("nope")

    def it_wraps_a_find_spec_error():
        find_spec, patterns, globbing = _resolving(find_spec_error=ImportError("boom"))
        with (
            find_spec,
            patterns,
            globbing,
            pytest.raises(
                ValueError, match=r"could not resolve extension package 'nope': boom"
            ),
        ):
            mod._resolve_package("nope")

    def it_wraps_a_find_spec_value_error():
        find_spec, patterns, globbing = _resolving(find_spec_error=ValueError("bad"))
        with (
            find_spec,
            patterns,
            globbing,
            pytest.raises(
                ValueError, match=r"could not resolve extension package 'nope': bad"
            ),
        ):
            mod._resolve_package("nope")

    def it_errors_when_the_spec_has_no_package_directory():
        # A namespace-less builtin/frozen module, and one with no origin at
        # all, are equally unresolvable -- neither names a directory to glob.
        for origin in ("built-in", "frozen", None):
            find_spec, patterns, globbing = _resolving(spec=_spec(origin=origin))
            with (
                find_spec,
                patterns,
                globbing,
                pytest.raises(
                    ValueError,
                    match=(
                        r"could not resolve extension package 'nope': "
                        r"no package directory"
                    ),
                ),
            ):
                mod._resolve_package("nope")


def describe_ambiguous_matches():
    def it_errors_when_no_loadable_file_is_found():
        find_spec, patterns, globbing = _resolving(
            spec=_spec(locations=["/site/x"]),
            patterns=("*.dll", "*.pyd"),
            glob={"return_value": []},
        )
        with (
            find_spec,
            patterns,
            globbing,
            pytest.raises(
                ValueError,
                match=(
                    r"no loadable extension file \(\*\.dll / \*\.pyd\) found in "
                    r"package 'x' \(searched /site/x\)"
                ),
            ),
        ):
            mod._resolve_package("x")

    def it_errors_when_multiple_loadable_files_are_found():
        # The listing is sorted, so the message is stable whatever order the
        # globs returned the matches in.
        find_spec, patterns, globbing = _resolving(
            spec=_spec(locations=["/site/x"]),
            glob={"return_value": ["/site/x/b.so", "/site/x/a.so"]},
        )
        with (
            find_spec,
            patterns,
            globbing,
            pytest.raises(
                ValueError,
                match=(
                    r"multiple loadable extension files found in package 'x': "
                    r"/site/x/a\.so, /site/x/b\.so; disambiguate with a literal path"
                ),
            ),
        ):
            mod._resolve_package("x")
