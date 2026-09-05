"""Unit tests for `resolve_extension_path`.

The effectful collaborators -- the filesystem probe (`os.path.isfile`) and the
package resolver -- are mocked, so these isolate the ordered probe from any
real package or disk. The pure `is_bare_name` stays real.
"""

from unittest import mock

import dirsql.resolve_extension as mod


def describe_path_looking_values():
    def it_makes_a_relative_path_absolute_when_resolve_relative():
        out = mod.resolve_extension_path("ext/a.so", base="/cfg", resolve_relative=True)
        assert out == "/cfg/ext/a.so"

    def it_preserves_an_absolute_path_when_resolve_relative():
        out = mod.resolve_extension_path(
            "/abs/a.so", base="/cfg", resolve_relative=True
        )
        assert out == "/abs/a.so"

    def it_returns_a_path_verbatim_when_not_resolve_relative():
        out = mod.resolve_extension_path(
            "relative/a.so", base="/cfg", resolve_relative=False
        )
        assert out == "relative/a.so"


def describe_bare_names():
    def it_uses_a_same_named_local_file_when_present():
        with (
            mock.patch.object(mod.os.path, "isfile", return_value=True) as isfile,
            mock.patch.object(mod, "_resolve_package") as resolve_package,
        ):
            out = mod.resolve_extension_path("vec", base="/cfg", resolve_relative=True)
        assert out == "/cfg/vec"
        isfile.assert_called_once_with("/cfg/vec")
        resolve_package.assert_not_called()

    def it_resolves_the_package_when_no_local_file_shadows_it():
        with (
            mock.patch.object(mod.os.path, "isfile", return_value=False),
            mock.patch.object(
                mod, "_resolve_package", return_value="/site/vec/vec0.so"
            ) as resolve_package,
        ):
            out = mod.resolve_extension_path("vec", base="/cfg", resolve_relative=True)
        assert out == "/site/vec/vec0.so"
        resolve_package.assert_called_once_with("vec")

    def it_probes_the_shadow_file_even_when_not_resolve_relative():
        with (
            mock.patch.object(mod.os.path, "isfile", return_value=True) as isfile,
            mock.patch.object(mod, "_resolve_package"),
        ):
            out = mod.resolve_extension_path("vec", base="/cfg", resolve_relative=False)
        assert out == "/cfg/vec"
        isfile.assert_called_once_with("/cfg/vec")
