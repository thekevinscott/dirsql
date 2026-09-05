"""Unit tests for `_platform_patterns`.

`sys.platform` is mocked, so the dispatch is exercised on every host.
"""

from unittest import mock

import dirsql.platform_patterns as mod


def _patterns_on(platform):
    with mock.patch.object(mod.sys, "platform", platform):
        return mod._platform_patterns()


def describe_platform_patterns():
    def it_globs_dylib_on_macos():
        assert _patterns_on("darwin") == ("*.dylib",)

    def it_globs_dll_and_pyd_on_windows():
        assert _patterns_on("win32") == ("*.dll", "*.pyd")

    def it_globs_so_on_any_other_platform():
        # aix sorts below "darwin" and zos above "win32", pinning the
        # dispatch to equality rather than any ordering.
        for platform in ("aix", "zos", "linux"):
            assert _patterns_on(platform) == ("*.so",)

    def it_dispatches_on_platform_string_equality_not_identity():
        # Runtime-built platform strings (not interned literals) must still
        # dispatch correctly -- `sys.platform` is compared by value.
        assert _patterns_on("".join(["dar", "win"])) == ("*.dylib",)
        assert _patterns_on("".join(["win", "32"])) == ("*.dll", "*.pyd")
