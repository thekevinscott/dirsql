"""Integration tests for SQLite extension loading via the Python SDK (#229).

The Python `DirSQL(extensions=[...])` constructor argument (and the
`[[dirsql.extension]]` config-file form) marshal into the shared Rust core,
which loads each extension onto the connection at startup -- before any
`CREATE TABLE` -- then disables loading again.

These exercise the public SDK surface: a *missing* extension must fail
construction, proving the path reaches the core's load-at-startup logic. A
real-extension end-to-end load is covered by the Rust suite
(`packages/rust/tests/extensions.rs`), which builds a fixture `.so`; the
binding here is a thin marshal to that same core.
"""

import os

import pytest

from dirsql import DirSQL, Table


def _noop_extract(_path):
    return []


def describe_constructor_extensions():
    @pytest.mark.asyncio
    async def it_raises_when_a_constructor_extension_is_missing(tmp_dir):
        """A nonexistent `extensions=` path fails the background build, and the
        error -- naming the library -- is re-raised by `await ready()`."""
        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    ddl="CREATE TABLE items (name TEXT)",
                    glob="*.json",
                    extract=_noop_extract,
                )
            ],
            extensions=[{"path": "/nonexistent/dirsql-no-such-ext.so"}],
        )
        with pytest.raises(Exception) as excinfo:
            await db.ready()
        assert "failed to load extension" in str(excinfo.value)

    @pytest.mark.asyncio
    async def it_accepts_an_optional_entrypoint(tmp_dir):
        """An `entrypoint` override is carried into the load call; a missing
        library still fails, proving the entry reached the core verbatim."""
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
                    extract=lambda _path: [{"name": "x"}],
                )
            ],
        )
        await db.ready()
        assert await db.query("SELECT COUNT(*) AS n FROM items") == [{"n": 1}]


def describe_config_file_extensions():
    @pytest.mark.asyncio
    async def it_raises_when_a_config_file_extension_is_missing(tmp_dir):
        """A `[[dirsql.extension]]` entry in a `.dirsql.toml` passed via
        `config=` is loaded by the core -- a missing library fails the build,
        confirming Python's `config=` path delegates extension loading to the
        shared Rust config loader."""
        cfg_path = os.path.join(tmp_dir, ".dirsql.toml")
        with open(cfg_path, "w") as f:
            f.write('[[dirsql.extension]]\npath = "missing-extension.so"\n')
        db = DirSQL(config=cfg_path)
        with pytest.raises(Exception) as excinfo:
            await db.ready()
        assert "failed to load extension" in str(excinfo.value)
