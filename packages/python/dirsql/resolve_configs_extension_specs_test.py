"""Unit tests for `resolve_configs_extension_specs`.

Real `.dirsql.toml` files are parsed; `_resolve_entries` (which owns the
effectful per-entry resolution) is mocked, returning a `base:path` sentinel
per entry so ordering and the per-config base directory are both observable.
"""

import os
import tempfile
from unittest import mock

import pytest

import dirsql.resolve_configs_extension_specs as mod


@pytest.fixture
def cfg_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _write(path, body):
    with open(path, "w") as f:
        f.write(body)


def _patch():
    return mock.patch.object(
        mod,
        "_resolve_entries",
        side_effect=lambda entries, base: [f"{base}:{e['path']}" for e in entries],
    )


def describe_resolve_configs_extension_specs():
    def it_returns_none_when_no_config_uses_a_bare_name(cfg_dir):
        a = os.path.join(cfg_dir, "a.toml")
        b = os.path.join(cfg_dir, "b.toml")
        _write(a, '[[dirsql.extension]]\npath = "ext/a.so"\n')
        _write(
            b, '[[table]]\nname = "t"\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*"\n'
        )
        with _patch() as resolve_entries:
            assert mod.resolve_configs_extension_specs([a, b]) is None
            resolve_entries.assert_not_called()

    def it_returns_none_for_an_empty_list():
        assert mod.resolve_configs_extension_specs([]) is None

    def it_resolves_every_config_in_order_when_one_uses_a_bare_name(cfg_dir):
        a = os.path.join(cfg_dir, "a.toml")
        b = os.path.join(cfg_dir, "b.toml")
        # a: only a literal; b: a bare package name -> intervene for both.
        _write(a, '[[dirsql.extension]]\npath = "ext/a.so"\n')
        _write(b, '[[dirsql.extension]]\npath = "sqlite_vec"\n')
        with _patch():
            specs = mod.resolve_configs_extension_specs([a, b])
        assert specs == [f"{cfg_dir}:ext/a.so", f"{cfg_dir}:sqlite_vec"]

    def it_skips_a_missing_config_but_resolves_the_rest(cfg_dir):
        missing = os.path.join(cfg_dir, "nope.toml")
        b = os.path.join(cfg_dir, "b.toml")
        _write(b, '[[dirsql.extension]]\npath = "sqlite_vec"\n')
        with _patch():
            specs = mod.resolve_configs_extension_specs([missing, b])
        assert specs == [f"{cfg_dir}:sqlite_vec"]

    def it_resolves_each_config_against_its_own_directory(cfg_dir):
        with tempfile.TemporaryDirectory() as other:
            a = os.path.join(cfg_dir, "a.toml")
            b = os.path.join(other, "b.toml")
            _write(a, '[[dirsql.extension]]\npath = "sqlite_vec"\n')
            _write(b, '[[dirsql.extension]]\npath = "ext/b.so"\n')
            with _patch():
                specs = mod.resolve_configs_extension_specs([a, b])
            assert specs == [f"{cfg_dir}:sqlite_vec", f"{other}:ext/b.so"]
