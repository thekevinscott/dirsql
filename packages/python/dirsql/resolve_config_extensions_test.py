"""Unit tests for `resolve_config_extension_specs`.

Real `.dirsql.toml` files are parsed; `_resolve_entries` (which owns the
effectful per-entry resolution) is mocked, returning a `base:path` sentinel
per entry. The pure `is_bare_name` stays real.
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
        "_resolve_entries",
        side_effect=lambda entries, base: [f"{base}:{e['path']}" for e in entries],
    )


def describe_load_toml_module():
    # `import_module` is mocked and `version_info` patched so both arms run on
    # every interpreter -- neither `tomllib` (absent on 3.10) nor `tomli`
    # (absent on 3.11+) is importable everywhere.
    def _requested(version):
        with (
            mock.patch.object(sys, "version_info", version),
            mock.patch.object(rce, "import_module") as import_module,
        ):
            assert rce._load_toml_module() is import_module.return_value
        (name,) = import_module.call_args.args
        return name

    # Exactly `(3, 11)`, not `(3, 11, 0)`: the latter compares greater than the
    # bare `(3, 11)` literal, so it cannot tell `>=` from `>`.
    def it_uses_the_stdlib_parser_on_the_version_that_gained_it():
        assert _requested((3, 11)) == "tomllib"

    def it_uses_the_stdlib_parser_on_newer_versions():
        assert _requested((3, 12, 4)) == "tomllib"

    def it_uses_the_tomli_backport_below_311():
        assert _requested((3, 10, 17)) == "tomli"


def describe_resolve_config_extension_specs():
    def it_returns_none_when_the_config_is_missing():
        assert rce.resolve_config_extension_specs("/nope/.dirsql.toml") is None

    def it_returns_none_on_malformed_toml(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, "this is not = valid = toml")
        assert rce.resolve_config_extension_specs(path) is None

    def it_returns_none_when_there_is_no_dirsql_section(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(
            path, '[[table]]\nname = "t"\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*"\n'
        )
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
        with _patch() as resolve_entries:
            assert rce.resolve_config_extension_specs(path) is None
            resolve_entries.assert_not_called()

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
        with _patch() as resolve_entries:
            specs = rce.resolve_config_extension_specs(path)
        assert specs == [f"{cfg_dir}:sqlite_vec", f"{cfg_dir}:ext/local.so"]
        resolve_entries.assert_called_once_with(
            [
                {"path": "sqlite_vec", "entrypoint": "sqlite3_vec_init"},
                {"path": "ext/local.so"},
            ],
            cfg_dir,
        )

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
        assert specs == [f"{cfg_dir}:42", f"{cfg_dir}:sqlite_vec"]
