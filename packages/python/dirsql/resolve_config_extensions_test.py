"""Unit tests for `resolve_config_extension_specs`.

Real `.dirsql.toml` files are parsed; the only mocked collaborator is
`resolve_extension_path` (the effectful file-vs-package resolver). The pure
`is_bare_name` stays real.
"""

import os
import sys
import tempfile
from unittest import mock

import pytest

import dirsql.resolve_config_extensions as rce


@pytest.fixture
def cfg_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _write(path, body):
    with open(path, "w") as f:
        f.write(body)


def _patch():
    return mock.patch.object(
        rce,
        "resolve_extension_path",
        side_effect=lambda path, base, resolve_relative: f"R:{path}",
    )


def describe_load_toml_module():
    # `version_info` is patched rather than skipping per interpreter: both arms
    # must be exercised on every CI leg, and `tomli` is a dev dependency on all
    # versions so the backport arm really imports.
    def _under(version):
        with mock.patch.object(sys, "version_info", version):
            return rce._load_toml_module().__name__

    def it_uses_the_stdlib_parser_on_the_version_that_gained_it():
        assert _under((3, 11, 0)) == "tomllib"

    def it_uses_the_stdlib_parser_on_newer_versions():
        assert _under((3, 12, 4)) == "tomllib"

    def it_uses_the_tomli_backport_below_311():
        assert _under((3, 10, 17)) == "tomli"


def describe_resolve_config_extension_specs():
    def it_returns_none_when_the_config_is_missing():
        assert rce.resolve_config_extension_specs("/nope/.dirsql.toml") is None

    def it_returns_none_on_malformed_toml(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, "this is not = valid = toml")
        assert rce.resolve_config_extension_specs(path) is None

    def it_returns_none_when_there_is_no_dirsql_section(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*"\n')
        assert rce.resolve_config_extension_specs(path) is None

    def it_returns_none_when_no_extensions_are_declared(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[dirsql]\nignore = ["x"]\n')
        assert rce.resolve_config_extension_specs(path) is None

    def it_returns_none_when_extension_is_not_a_list(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[dirsql]\nextension = "not-a-list"\n')
        assert rce.resolve_config_extension_specs(path) is None

    def it_returns_none_when_extension_entries_are_not_tables(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, "[dirsql]\nextension = [1, 2]\n")
        assert rce.resolve_config_extension_specs(path) is None

    def it_returns_none_when_all_paths_are_literal(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[[dirsql.extension]]\npath = "ext/a.so"\n')
        with _patch() as resolver:
            assert rce.resolve_config_extension_specs(path) is None
            resolver.assert_not_called()

    def it_resolves_every_entry_when_a_path_is_a_bare_package_name(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(
            path,
            "[[dirsql.extension]]\n"
            'path = "sqlite_vec"\n'
            'entrypoint = "sqlite3_vec_init"\n\n'
            "[[dirsql.extension]]\n"
            'path = "ext/local.so"\n',
        )
        with _patch() as resolver:
            specs = rce.resolve_config_extension_specs(path)
        assert specs == [
            {"path": "R:sqlite_vec", "entrypoint": "sqlite3_vec_init"},
            {"path": "R:ext/local.so", "entrypoint": None},
        ]
        resolver.assert_any_call("sqlite_vec", base=cfg_dir, resolve_relative=True)
        resolver.assert_any_call("ext/local.so", base=cfg_dir, resolve_relative=True)

    def it_normalizes_a_non_string_entrypoint_to_none(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(
            path,
            '[[dirsql.extension]]\npath = "sqlite_vec"\nentrypoint = 42\n',
        )
        with _patch():
            specs = rce.resolve_config_extension_specs(path)
        assert specs == [{"path": "R:sqlite_vec", "entrypoint": None}]

    def it_skips_a_non_string_path_in_the_package_name_probe(cfg_dir):
        # A dict entry whose `path` isn't a string is not treated as a package
        # name; a sibling bare name still triggers resolution of every entry.
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(
            path,
            "[[dirsql.extension]]\npath = 42\n\n"
            '[[dirsql.extension]]\npath = "sqlite_vec"\n',
        )
        with _patch():
            specs = rce.resolve_config_extension_specs(path)
        assert specs == [
            {"path": "R:42", "entrypoint": None},
            {"path": "R:sqlite_vec", "entrypoint": None},
        ]


def describe_resolve_configs_extension_specs():
    def it_returns_none_when_no_config_uses_a_bare_name(cfg_dir):
        a = os.path.join(cfg_dir, "a.toml")
        b = os.path.join(cfg_dir, "b.toml")
        _write(a, '[[dirsql.extension]]\npath = "ext/a.so"\n')
        _write(b, '[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*"\n')
        with _patch() as resolver:
            assert rce.resolve_configs_extension_specs([a, b]) is None
            resolver.assert_not_called()

    def it_returns_none_for_an_empty_list():
        assert rce.resolve_configs_extension_specs([]) is None

    def it_resolves_every_config_in_order_when_one_uses_a_bare_name(cfg_dir):
        a = os.path.join(cfg_dir, "a.toml")
        b = os.path.join(cfg_dir, "b.toml")
        # a: only a literal; b: a bare package name -> intervene for both.
        _write(a, '[[dirsql.extension]]\npath = "ext/a.so"\n')
        _write(b, '[[dirsql.extension]]\npath = "sqlite_vec"\n')
        with _patch():
            specs = rce.resolve_configs_extension_specs([a, b])
        assert specs == [
            {"path": "R:ext/a.so", "entrypoint": None},
            {"path": "R:sqlite_vec", "entrypoint": None},
        ]

    def it_skips_a_missing_config_but_resolves_the_rest(cfg_dir):
        missing = os.path.join(cfg_dir, "nope.toml")
        b = os.path.join(cfg_dir, "b.toml")
        _write(b, '[[dirsql.extension]]\npath = "sqlite_vec"\n')
        with _patch():
            specs = rce.resolve_configs_extension_specs([missing, b])
        assert specs == [{"path": "R:sqlite_vec", "entrypoint": None}]

    def it_resolves_each_config_against_its_own_directory(cfg_dir):
        import tempfile

        with tempfile.TemporaryDirectory() as other:
            a = os.path.join(cfg_dir, "a.toml")
            b = os.path.join(other, "b.toml")
            _write(a, '[[dirsql.extension]]\npath = "sqlite_vec"\n')
            _write(b, '[[dirsql.extension]]\npath = "ext/b.so"\n')
            with mock.patch.object(
                rce,
                "resolve_extension_path",
                side_effect=lambda path, base, resolve_relative: base,
            ):
                specs = rce.resolve_configs_extension_specs([a, b])
            assert specs == [
                {"path": cfg_dir, "entrypoint": None},
                {"path": other, "entrypoint": None},
            ]
