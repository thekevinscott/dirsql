"""Unit tests for `_load_extension_entries`.

Real `.dirsql.toml` files under a temp dir are parsed by the real TOML module:
the unit under test is the shape-checking around the parse, and a stub parser
would only restate it. The unreadable-file arm mocks `open`.
"""

import os
import tempfile
from unittest import mock

import pytest

import dirsql.load_extension_entries as mod


@pytest.fixture
def cfg_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _write(cfg_dir, body):
    path = os.path.join(cfg_dir, ".dirsql.toml")
    with open(path, "w") as f:
        f.write(body)
    return path


def describe_configs_with_no_usable_extension_array():
    def it_returns_none_when_the_config_is_missing():
        assert mod._load_extension_entries("/nope/.dirsql.toml") is None

    def it_returns_none_on_malformed_toml(cfg_dir):
        path = _write(cfg_dir, "this is not = valid = toml")
        assert mod._load_extension_entries(path) is None

    def it_returns_none_when_the_config_cannot_be_read(cfg_dir):
        path = _write(cfg_dir, '[[dirsql.extension]]\npath = "vec"\n')
        with mock.patch("builtins.open", side_effect=OSError("denied")):
            assert mod._load_extension_entries(path) is None

    def it_returns_none_when_there_is_no_dirsql_section(cfg_dir):
        path = _write(
            cfg_dir,
            '[[table]]\nname = "t"\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*"\n',
        )
        assert mod._load_extension_entries(path) is None

    def it_returns_none_when_no_extensions_are_declared(cfg_dir):
        path = _write(cfg_dir, '[dirsql]\nignore = ["x"]\n')
        assert mod._load_extension_entries(path) is None

    def it_returns_none_when_extension_is_not_a_list(cfg_dir):
        path = _write(cfg_dir, '[dirsql]\nextension = "not-a-list"\n')
        assert mod._load_extension_entries(path) is None


def describe_configs_declaring_extensions():
    def it_returns_the_entries_and_the_configs_own_directory(cfg_dir):
        path = _write(
            cfg_dir,
            "[[dirsql.extension]]\n"
            'path = "sqlite_vec"\n'
            'entrypoint = "sqlite3_vec_init"\n\n'
            "[[dirsql.extension]]\n"
            'path = "ext/local.so"\n',
        )
        assert mod._load_extension_entries(path) == (
            [
                {"path": "sqlite_vec", "entrypoint": "sqlite3_vec_init"},
                {"path": "ext/local.so"},
            ],
            cfg_dir,
        )

    def it_returns_entries_that_are_not_tables_verbatim(cfg_dir):
        # Shape-checking each entry is the caller's job; this only asserts the
        # array is a list.
        path = _write(cfg_dir, "[dirsql]\nextension = [1, 2]\n")
        assert mod._load_extension_entries(path) == ([1, 2], cfg_dir)

    def it_normalizes_the_base_directory(cfg_dir):
        # A traversal segment in the config path must not survive into the
        # base directory every extension is then resolved against.
        sub = os.path.join(cfg_dir, "sub")
        os.mkdir(sub)
        _write(sub, '[[dirsql.extension]]\npath = "vec"\n')
        _, base = mod._load_extension_entries(
            os.path.join(cfg_dir, "sub", "..", "sub", ".dirsql.toml")
        )
        assert base == sub
