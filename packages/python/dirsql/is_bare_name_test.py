"""Unit tests for `is_bare_name`.

`os.sep` / `os.altsep` are mocked where a non-posix separator matters, so the
alternate-separator arm runs on every host.
"""

from unittest import mock

import dirsql.is_bare_name as mod


def describe_loadable_suffixes():
    def it_lists_every_loadable_extension_suffix():
        assert mod._LOADABLE_SUFFIXES == (".so", ".dylib", ".dll", ".pyd")


def describe_is_bare_name():
    def it_treats_a_separatorful_value_as_a_path():
        assert mod.is_bare_name("ext/vec0.so") is False
        assert mod.is_bare_name("/abs/ext.so") is False

    def it_treats_a_separatorful_value_without_a_loadable_suffix_as_a_path():
        assert mod.is_bare_name("ext/foo") is False
        assert mod.is_bare_name("/abs/dir") is False

    def it_treats_an_alternate_separator_as_a_path():
        with (
            mock.patch.object(mod.os, "sep", "\\"),
            mock.patch.object(mod.os, "altsep", "/"),
        ):
            assert mod.is_bare_name("ext/foo") is False
            assert mod.is_bare_name("ext\\foo") is False
            assert mod.is_bare_name("sqlite_vec") is True

    def it_treats_a_loadable_suffix_as_a_path():
        assert mod.is_bare_name("vec0.so") is False
        assert mod.is_bare_name("vec0.dylib") is False
        assert mod.is_bare_name("vec0.dll") is False
        assert mod.is_bare_name("vec0.pyd") is False

    def it_treats_a_plain_identifier_as_a_bare_name():
        assert mod.is_bare_name("sqlite_vec") is True
