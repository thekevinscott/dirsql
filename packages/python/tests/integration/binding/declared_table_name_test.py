"""Binding-tier tests (real core, real fs) for the declared table `name`.

A table's name is *declared*, never derived from ``ddl``. ``Table`` takes it
as a required keyword argument, a ``[[table]]`` entry without ``name`` fails
to load, and a ``name`` the entry's ``ddl`` does not actually create fails at
load time -- checked against SQLite's own catalog, before any ingestion.
"""

import os
import tempfile

import pytest

from dirsql import DirSQL, Table

# An `on-file` hook emitting the file's root-relative `path`.
_HOOK_PATH = r"""on-file = '''sh -c 'rel=${1#"$2"/}; printf "[{\"path\":\"%s\"}]" "$rel"' sh {path} {root}'''"""


@pytest.fixture
def config_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _fixture(config_dir, config):
    os.makedirs(os.path.join(config_dir, "data"), exist_ok=True)
    with open(os.path.join(config_dir, "data", "a.csv"), "w") as f:
        f.write("anything")
    path = os.path.join(config_dir, ".dirsql.toml")
    with open(path, "w") as f:
        f.write(config)
    return path


def describe_declared_table_name():
    def it_exposes_the_declared_name_on_the_table():
        table = Table(
            name="notes",
            ddl='CREATE TABLE "notes" (path TEXT)',
            glob="data/*.csv",
            on_file=lambda path: [],
        )
        assert table.name == "notes"

    @pytest.mark.asyncio
    async def it_registers_a_config_table_under_its_declared_name(config_dir):
        cfg = _fixture(
            config_dir,
            """\
[[table]]
name = "notes"
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
"""
            + _HOOK_PATH
            + "\n",
        )

        db = DirSQL(root=config_dir, config=cfg)
        await db.ready()
        results = await db.query("SELECT path FROM notes")
        assert [r["path"] for r in results] == ["data/a.csv"]

    @pytest.mark.asyncio
    async def it_errors_when_a_table_entry_has_no_name(config_dir):
        cfg = _fixture(
            config_dir,
            """\
[[table]]
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
"""
            + _HOOK_PATH
            + "\n",
        )

        db = DirSQL(root=config_dir, config=cfg)
        with pytest.raises(Exception, match="name"):
            await db.ready()

    @pytest.mark.asyncio
    async def it_errors_when_the_ddl_never_creates_the_declared_name(config_dir):
        cfg = _fixture(
            config_dir,
            """\
[[table]]
name = "messages"
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
"""
            + _HOOK_PATH
            + "\n",
        )

        db = DirSQL(root=config_dir, config=cfg)
        with pytest.raises(Exception, match="table 'messages'"):
            await db.ready()
