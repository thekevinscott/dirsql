"""Unit tests for `resolve_config`."""

import os
import tempfile

import pytest

from dirsql.resolve_config import resolve_config


class _FakeTable:
    """Stand-in for the PyO3 `Table` class. `resolve_config` only reads
    `ddl`, `glob`, `strict`, so we don't need the real binding here."""

    def __init__(self, ddl, glob, strict=False):
        self.ddl = ddl
        self.glob = glob
        self.strict = strict


@pytest.fixture
def cfg_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _write(path, body):
    with open(path, "w") as f:
        f.write(body)


def describe_resolve_config():
    def describe_without_a_config_file():
        def it_forwards_the_root_kwarg(cfg_dir):
            out = resolve_config("/abs/data", None, None, None, False, None)
            assert out == {
                "root": "/abs/data",
                "tables": [],
                "ignore": [],
                "persist": False,
                "persist_path": None,
            }

        def it_serializes_a_programmatic_table_with_default_strict():
            out = resolve_config(
                "/r",
                [_FakeTable("x", "*")],
                None,
                None,
                False,
                None,
            )
            assert out["tables"] == [{"ddl": "x", "glob": "*", "strict": False}]

        def it_serializes_a_programmatic_table_with_strict_true():
            out = resolve_config(
                "/r",
                [_FakeTable("x", "*", strict=True)],
                None,
                None,
                False,
                None,
            )
            assert out["tables"][0]["strict"] is True

        def it_forwards_the_ignore_kwarg():
            out = resolve_config("/r", None, ["**/skip/**"], None, False, None)
            assert out["ignore"] == ["**/skip/**"]

        def it_forwards_persist_and_persist_path_kwargs():
            out = resolve_config(
                "/r",
                None,
                None,
                None,
                True,
                "/abs/cache.db",
            )
            assert out["persist"] is True
            assert out["persist_path"] == "/abs/cache.db"

    def describe_with_a_config_file():
        def it_resolves_a_relative_dirsql_root_against_the_config_parent(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\nroot = "data"\n')
            out = resolve_config(None, None, None, path, False, None)
            assert out["root"] == os.path.join(cfg_dir, "data")
            assert os.path.isabs(out["root"])

        def it_preserves_an_absolute_dirsql_root_verbatim(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\nroot = "/other/abs/path"\n')
            out = resolve_config(None, None, None, path, False, None)
            assert out["root"] == "/other/abs/path"

        def it_defaults_root_to_config_parent_when_dirsql_root_is_absent(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\nignore = ["x"]\n')
            out = resolve_config(None, None, None, path, False, None)
            assert out["root"] == cfg_dir

        def it_defaults_root_to_config_parent_when_dirsql_section_is_absent(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(
                path, '[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*.json"\n'
            )
            out = resolve_config(None, None, None, path, False, None)
            assert out["root"] == cfg_dir

        def it_reads_table_entries_with_strict_defaulting_to_false(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(
                path,
                '[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*.json"\n',
            )
            out = resolve_config(None, None, None, path, False, None)
            assert out["tables"] == [
                {"ddl": "CREATE TABLE t (x TEXT)", "glob": "*.json", "strict": False}
            ]

        def it_respects_strict_true_on_a_table_entry(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(
                path,
                '[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*.json"\nstrict = true\n',
            )
            out = resolve_config(None, None, None, path, False, None)
            assert out["tables"][0]["strict"] is True

        def it_returns_empty_tables_when_no_table_entries_declared(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, "[dirsql]\nignore = []\n")
            out = resolve_config(None, None, None, path, False, None)
            assert out["tables"] == []

        def it_forwards_dirsql_ignore(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\nignore = ["node_modules/**"]\n')
            out = resolve_config(None, None, None, path, False, None)
            assert out["ignore"] == ["node_modules/**"]

        def it_flips_persist_on_when_dirsql_persist_is_true(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, "[dirsql]\npersist = true\n")
            out = resolve_config(None, None, None, path, False, None)
            assert out["persist"] is True

        def it_resolves_a_relative_persist_path_against_the_config_dir(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(
                path,
                '[dirsql]\npersist = true\npersist_path = "cache/dirsql.db"\n',
            )
            out = resolve_config(None, None, None, path, False, None)
            assert out["persist_path"] == os.path.join(cfg_dir, "cache/dirsql.db")

        def it_preserves_an_absolute_persist_path_verbatim(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\npersist_path = "/var/cache/dirsql.db"\n')
            out = resolve_config(None, None, None, path, False, None)
            assert out["persist_path"] == "/var/cache/dirsql.db"

        def it_leaves_persist_path_none_when_absent(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\nignore = ["x"]\n')
            out = resolve_config(None, None, None, path, False, None)
            assert out["persist_path"] is None

    def describe_merging_kwargs_with_a_config_file():
        def it_explicit_root_wins_over_dirsql_root(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\nroot = "from-config"\n')
            out = resolve_config("/from-kwarg", None, None, path, False, None)
            assert out["root"] == "/from-kwarg"

        def it_concatenates_programmatic_then_config_tables(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(
                path,
                '[[table]]\nddl = "CREATE TABLE c (x TEXT)"\nglob = "c/*"\n',
            )
            out = resolve_config(
                "/r",
                [_FakeTable("p-ddl", "p/*")],
                None,
                path,
                False,
                None,
            )
            assert [t["ddl"] for t in out["tables"]] == [
                "p-ddl",
                "CREATE TABLE c (x TEXT)",
            ]

        def it_concatenates_ignore_kwargs_first_then_config_ignore(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\nignore = ["from-config/**"]\n')
            out = resolve_config(
                "/r",
                None,
                ["from-kwarg/**"],
                path,
                False,
                None,
            )
            assert out["ignore"] == ["from-kwarg/**", "from-config/**"]

        def it_ors_persist_across_kwarg_true_and_config_absent(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\nignore = ["x"]\n')
            out = resolve_config("/r", None, None, path, True, None)
            assert out["persist"] is True

        def it_ors_persist_across_kwarg_absent_and_config_true(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, "[dirsql]\npersist = true\n")
            out = resolve_config("/r", None, None, path, False, None)
            assert out["persist"] is True

        def it_explicit_persist_path_wins_over_config_persist_path(cfg_dir):
            path = os.path.join(cfg_dir, ".dirsql.toml")
            _write(path, '[dirsql]\npersist_path = "from-config.db"\n')
            out = resolve_config(
                "/r",
                None,
                None,
                path,
                False,
                "/from-kwarg.db",
            )
            assert out["persist_path"] == "/from-kwarg.db"
