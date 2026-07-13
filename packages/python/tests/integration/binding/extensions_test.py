"""Binding-tier tests (real core) for SQLite extension loading via the Python SDK.

`DirSQL(extensions=[...])` and the `[[dirsql.extension]]` config-file form
marshal into the shared Rust core, which loads each extension onto the
connection at startup -- before any `CREATE TABLE` -- then disables loading
again. A real-extension end-to-end load is covered by the Rust suite
(`packages/rust/tests/extensions.rs`).
"""

import os

import pytest

from dirsql import DirSQL, Table


def _noop_extract(_path):
    return []


def describe_constructor_extensions():
    @pytest.mark.asyncio
    async def it_raises_when_a_constructor_extension_is_missing(tmp_dir):
        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT)",
                    glob="*.json",
                    on_file=_noop_extract,
                )
            ],
            extensions=[{"path": "/nonexistent/dirsql-no-such-ext.so"}],
        )
        with pytest.raises(Exception) as excinfo:
            await db.ready()
        assert "failed to load extension" in str(excinfo.value)

    @pytest.mark.asyncio
    async def it_accepts_an_optional_entrypoint(tmp_dir):
        db = DirSQL(
            tmp_dir,
            extensions=[
                {"path": "/nonexistent/dirsql-y.so", "entrypoint": "sqlite3_y_init"}
            ],
        )
        with pytest.raises(Exception) as excinfo:
            await db.ready()
        assert "failed to load extension" in str(excinfo.value)

    @pytest.mark.asyncio
    async def it_builds_normally_when_no_extensions_are_given(tmp_dir):
        with open(os.path.join(tmp_dir, "a.json"), "w") as f:
            f.write("{}")
        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT)",
                    glob="*.json",
                    on_file=lambda _path: [{"name": "x"}],
                )
            ],
        )
        await db.ready()
        assert await db.query("SELECT COUNT(*) AS n FROM items") == [{"n": 1}]


def describe_config_file_extensions():
    @pytest.mark.asyncio
    async def it_raises_when_a_config_file_extension_is_missing(tmp_dir):
        cfg_path = os.path.join(tmp_dir, ".dirsql.toml")
        with open(cfg_path, "w") as f:
            f.write('[[dirsql.extension]]\npath = "missing-extension.so"\n')
        db = DirSQL(root=tmp_dir, config=cfg_path)
        with pytest.raises(Exception) as excinfo:
            await db.ready()
        assert "failed to load extension" in str(excinfo.value)
