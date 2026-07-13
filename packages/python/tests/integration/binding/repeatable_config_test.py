"""Binding-tier tests (real core, real fs) for repeatable ``config=`` (#588).

``DirSQL(config=...)`` accepts a list of ``.dirsql.toml`` paths that merge in
order (matching the Rust builder's repeatable ``.config()`` and the CLI's
repeatable ``-c``); a single string stays byte-identical to before.
"""

import os
import tempfile

import pytest

from dirsql import DirSQL


@pytest.fixture
def config_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _write(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(content)


def _table_config(name, glob):
    return f'[[table]]\nddl = "CREATE TABLE {name} (basename TEXT)"\nglob = "{glob}"\n'


def describe_repeatable_config():
    @pytest.mark.asyncio
    async def it_merges_tables_from_multiple_config_files(config_dir):
        _write(os.path.join(config_dir, "a.json"), "{}")
        cfg_a = os.path.join(config_dir, "a.dirsql.toml")
        cfg_b = os.path.join(config_dir, "b.dirsql.toml")
        _write(cfg_a, _table_config("alpha", "*.json"))
        _write(cfg_b, _table_config("beta", "*.json"))

        db = DirSQL(root=config_dir, config=[cfg_a, cfg_b])
        await db.ready()

        for table in ("alpha", "beta"):
            rows = await db.query(f"SELECT basename FROM {table}")
            assert [r["basename"] for r in rows] == ["a.json"], (
                f"table {table} from an accumulated config must be queryable"
            )

    @pytest.mark.asyncio
    async def it_accepts_a_single_config_string_unchanged(config_dir):
        _write(os.path.join(config_dir, "a.json"), "{}")
        cfg = os.path.join(config_dir, ".dirsql.toml")
        _write(cfg, _table_config("alpha", "*.json"))

        db = DirSQL(root=config_dir, config=cfg)
        await db.ready()
        rows = await db.query("SELECT basename FROM alpha")
        assert [r["basename"] for r in rows] == ["a.json"]
