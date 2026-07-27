"""Unit tests for `resolve_extension`.

Every collaborator -- `importlib.util.find_spec`, the filesystem probes
(`os.path.isfile`), the loadable glob (`glob.glob`), and `sys.platform` -- is
mocked, so these isolate the resolution logic from any real package or disk.
"""

import types
from unittest import mock

import pytest

import dirsql.resolve_extension as re


def _spec(*, locations=None, origin=None):
    return types.SimpleNamespace(submodule_search_locations=locations, origin=origin)


def describe_is_bare_name():
    def it_treats_a_separatorful_value_as_a_path():
        assert re.is_bare_name("ext/vec0.so") is False
        assert re.is_bare_name("/abs/ext.so") is False

    def it_treats_a_separatorful_value_without_a_loadable_suffix_as_a_path():
        assert re.is_bare_name("ext/foo") is False
        assert re.is_bare_name("/abs/dir") is False

    def it_treats_a_loadable_suffix_as_a_path():
        assert re.is_bare_name("vec0.so") is False
        assert re.is_bare_name("vec0.dylib") is False
        assert re.is_bare_name("vec0.dll") is False
        assert re.is_bare_name("vec0.pyd") is False

    def it_treats_a_plain_identifier_as_a_bare_name():
        assert re.is_bare_name("sqlite_vec") is True


def describe_path_looking_values():
    def it_makes_a_relative_path_absolute_when_resolve_relative(tmp_path):
        out = re.resolve_extension_path("ext/a.so", base="/cfg", resolve_relative=True)
        assert out == "/cfg/ext/a.so"

    def it_preserves_an_absolute_path_when_resolve_relative():
        out = re.resolve_extension_path("/abs/a.so", base="/cfg", resolve_relative=True)
        assert out == "/abs/a.so"

    def it_returns_a_path_verbatim_when_not_resolve_relative():
        out = re.resolve_extension_path(
            "relative/a.so", base="/cfg", resolve_relative=False
        )
        assert out == "relative/a.so"


def describe_bare_name_shadowing():
    def it_uses_a_same_named_local_file_when_present():
        with mock.patch.object(re.os.path, "isfile", return_value=True) as isfile:
            out = re.resolve_extension_path("vec", base="/cfg", resolve_relative=True)
        assert out == "/cfg/vec"
        isfile.assert_called_once_with("/cfg/vec")


def describe_bare_name_package_resolution():
    def it_globs_the_platform_loadable_inside_the_package_dir():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(re.sys, "platform", "linux"),
            mock.patch.object(
                re.importlib.util,
                "find_spec",
                return_value=_spec(locations=["/site/sqlite_vec"]),
            ),
            mock.patch.object(
                re._glob, "glob", return_value=["/site/sqlite_vec/vec0.so"]
            ) as glob,
        ):
            out = re.resolve_extension_path(
                "sqlite_vec", base="/cfg", resolve_relative=True
            )
        assert out == "/site/sqlite_vec/vec0.so"
        glob.assert_called_once_with("/site/sqlite_vec/**/*.so", recursive=True)

    def it_falls_back_to_origin_dir_for_a_single_file_module():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(re.sys, "platform", "linux"),
            mock.patch.object(
                re.importlib.util,
                "find_spec",
                return_value=_spec(origin="/site/sqlite_vec/__init__.py"),
            ),
            mock.patch.object(
                re._glob, "glob", return_value=["/site/sqlite_vec/vec0.so"]
            ),
        ):
            out = re.resolve_extension_path(
                "sqlite_vec", base="/cfg", resolve_relative=True
            )
        assert out == "/site/sqlite_vec/vec0.so"

    def it_globs_dylib_on_macos():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(re.sys, "platform", "darwin"),
            mock.patch.object(
                re.importlib.util,
                "find_spec",
                return_value=_spec(locations=["/site/x"]),
            ),
            mock.patch.object(
                re._glob, "glob", return_value=["/site/x/y.dylib"]
            ) as glob,
        ):
            out = re.resolve_extension_path("x", base="/c", resolve_relative=True)
        assert out == "/site/x/y.dylib"
        glob.assert_called_once_with("/site/x/**/*.dylib", recursive=True)

    def it_globs_dll_and_pyd_on_windows():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(re.sys, "platform", "win32"),
            mock.patch.object(
                re.importlib.util,
                "find_spec",
                return_value=_spec(locations=["/site/x"]),
            ),
            mock.patch.object(
                re._glob, "glob", side_effect=[["/site/x/y.dll"], []]
            ) as glob,
        ):
            out = re.resolve_extension_path("x", base="/c", resolve_relative=True)
        assert out == "/site/x/y.dll"
        assert glob.call_count == 2

    def it_globs_so_on_any_other_platform():
        # aix sorts below "darwin" and zos above "win32", pinning the
        # dispatch to equality rather than any ordering.
        for platform in ("aix", "zos", "linux"):
            with (
                mock.patch.object(re.os.path, "isfile", return_value=False),
                mock.patch.object(re.sys, "platform", platform),
                mock.patch.object(
                    re.importlib.util,
                    "find_spec",
                    return_value=_spec(locations=["/site/x"]),
                ),
                mock.patch.object(
                    re._glob, "glob", return_value=["/site/x/y.so"]
                ) as glob,
            ):
                out = re.resolve_extension_path("x", base="/c", resolve_relative=True)
            assert out == "/site/x/y.so"
            glob.assert_called_once_with("/site/x/**/*.so", recursive=True)

    def it_dispatches_on_platform_string_equality_not_identity():
        # Runtime-built platform strings (not interned literals) must still
        # dispatch correctly -- `sys.platform` is compared by value.
        for platform, pattern, loadable in (
            ("".join(["dar", "win"]), "*.dylib", "/site/x/y.dylib"),
            ("".join(["win", "32"]), "*.dll", "/site/x/y.dll"),
        ):
            with (
                mock.patch.object(re.os.path, "isfile", return_value=False),
                mock.patch.object(re.sys, "platform", platform),
                mock.patch.object(
                    re.importlib.util,
                    "find_spec",
                    return_value=_spec(locations=["/site/x"]),
                ),
                mock.patch.object(
                    re._glob, "glob", side_effect=[[loadable], []]
                ) as glob,
            ):
                out = re.resolve_extension_path("x", base="/c", resolve_relative=True)
            assert out == loadable
            assert glob.call_args_list[0].args[0] == f"/site/x/**/{pattern}", (
                glob.call_args_list
            )

    def it_errors_when_the_package_is_not_installed():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(re.importlib.util, "find_spec", return_value=None),
            pytest.raises(ValueError, match="not installed"),
        ):
            re.resolve_extension_path("nope", base="/c", resolve_relative=True)

    def it_wraps_a_find_spec_error():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(
                re.importlib.util, "find_spec", side_effect=ImportError("boom")
            ),
            pytest.raises(ValueError, match="could not resolve extension package"),
        ):
            re.resolve_extension_path("nope", base="/c", resolve_relative=True)

    def it_errors_when_the_spec_has_no_package_directory():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(
                re.importlib.util,
                "find_spec",
                return_value=_spec(origin="built-in"),
            ),
            pytest.raises(ValueError, match="no package directory"),
        ):
            re.resolve_extension_path("nope", base="/c", resolve_relative=True)

    def it_errors_when_no_loadable_file_is_found():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(re.sys, "platform", "linux"),
            mock.patch.object(
                re.importlib.util,
                "find_spec",
                return_value=_spec(locations=["/site/x"]),
            ),
            mock.patch.object(re._glob, "glob", return_value=[]),
            pytest.raises(ValueError, match="no loadable extension file"),
        ):
            re.resolve_extension_path("x", base="/c", resolve_relative=True)

    def it_errors_when_multiple_loadable_files_are_found():
        with (
            mock.patch.object(re.os.path, "isfile", return_value=False),
            mock.patch.object(re.sys, "platform", "linux"),
            mock.patch.object(
                re.importlib.util,
                "find_spec",
                return_value=_spec(locations=["/site/x"]),
            ),
            mock.patch.object(
                re._glob, "glob", return_value=["/site/x/a.so", "/site/x/b.so"]
            ),
            pytest.raises(ValueError, match="multiple loadable extension files"),
        ):
            re.resolve_extension_path("x", base="/c", resolve_relative=True)
