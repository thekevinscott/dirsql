"""Integration tests for DirSQL config serialization (issue #194).

A `DirSQL` instance exposes its resolved runtime state as a plain,
JSON-serializable dict via the standard Python `__dict__` property
(also reachable as `vars(app)`).

The serialized form captures resolved runtime state, not construction
parameters:

- `config` (the config-file path) is excluded -- by the time the instance
  exists the config file has been read and its contents merged into
  `root`, `tables`, and `ignore`.
- `extract` is excluded from the table shape -- closures are not
  serializable.
- `name` is excluded from the table shape.

Resolution happens synchronously in `__init__`, so `vars(db)` works
immediately without waiting for the initial directory scan to finish
(no need to `await db.ready()` first).
"""

import json
import os
import tempfile

import pytest

from dirsql import DirSQL, Table


@pytest.fixture
def empty_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _noop_extract(_path):
    return []


def describe_DirSQL_serialization():
    def describe_top_level_shape():
        @pytest.mark.asyncio
        async def it_exposes_resolved_state_via_vars(empty_dir):
            """`vars(db)` returns the resolved runtime state as a dict."""
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
                ignore=["**/skip/**"],
            )

            state = vars(db)
            assert isinstance(state, dict)
            assert set(state.keys()) == {
                "root",
                "tables",
                "ignore",
                "persist",
                "persist_path",
            }

        @pytest.mark.asyncio
        async def it_returns_json_serializable_dict(empty_dir):
            """`json.dumps(vars(db))` succeeds with the resolved state."""
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
            )

            payload = json.dumps(vars(db))
            parsed = json.loads(payload)
            assert parsed["root"] == empty_dir
            assert isinstance(parsed["tables"], list)
            assert len(parsed["tables"]) == 1
            assert parsed["tables"][0]["ddl"] == "CREATE TABLE items (name TEXT)"
            assert parsed["tables"][0]["glob"] == "items/*.json"
            assert parsed["tables"][0]["strict"] is False
            assert parsed["ignore"] == []
            assert parsed["persist"] is False
            assert parsed["persist_path"] is None

        @pytest.mark.asyncio
        async def it_works_before_ready(empty_dir):
            """`vars(db)` is available immediately, before the scan finishes."""
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
            )
            # Read state before awaiting ready -- must not raise.
            state = vars(db)
            assert state["root"] == empty_dir
            # And still works after ready, with the same shape.
            await db.ready()
            assert vars(db) == state

    def describe_table_shape():
        @pytest.mark.asyncio
        async def it_excludes_extract_from_tables(empty_dir):
            """Each serialized table must NOT include the `extract` closure."""
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
            )

            state = vars(db)
            assert state["tables"]
            for t in state["tables"]:
                assert "extract" not in t

        @pytest.mark.asyncio
        async def it_excludes_name_from_tables(empty_dir):
            """Each serialized table must NOT include a `name` field."""
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
            )

            state = vars(db)
            for t in state["tables"]:
                assert "name" not in t

    def describe_defaults():
        @pytest.mark.asyncio
        async def it_defaults_strict_to_false(empty_dir):
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
            )
            state = vars(db)
            assert state["tables"][0]["strict"] is False

        @pytest.mark.asyncio
        async def it_defaults_persist_to_false_and_persist_path_to_none(empty_dir):
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
            )
            state = vars(db)
            assert state["persist"] is False
            assert state["persist_path"] is None

    def describe_persist_overrides():
        @pytest.mark.asyncio
        async def it_reflects_persist_true_and_custom_persist_path(empty_dir):
            persist_path = os.path.join(empty_dir, "custom-cache.db")
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
                persist=True,
                persist_path=persist_path,
            )
            state = vars(db)
            assert state["persist"] is True
            assert state["persist_path"] == persist_path

    def describe_ignore_patterns():
        @pytest.mark.asyncio
        async def it_includes_ignore_list(empty_dir):
            db = DirSQL(
                empty_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="items/*.json",
                        extract=_noop_extract,
                    )
                ],
                ignore=["**/skip/**", "**/temp/**"],
            )
            state = vars(db)
            assert state["ignore"] == ["**/skip/**", "**/temp/**"]

    def describe_config_file():
        @pytest.mark.asyncio
        async def it_merges_root_tables_ignore_from_config(empty_dir):
            """A `.dirsql.toml` config feeds into the serialized state."""
            cfg_path = os.path.join(empty_dir, ".dirsql.toml")
            with open(cfg_path, "w") as f:
                f.write(
                    '[dirsql]\n'
                    'root = "data"\n'
                    'ignore = ["node_modules/**"]\n'
                    'persist = true\n'
                    'persist_path = "cache.db"\n'
                    '\n'
                    '[[table]]\n'
                    'ddl = "CREATE TABLE items (_path TEXT)"\n'
                    'glob = "*.json"\n'
                    'strict = true\n'
                )
            # Create the directory the config expects so the background
            # scan doesn't blow up later -- we don't await ready() here,
            # but the underlying Rust constructor still needs it on its
            # own thread.
            os.makedirs(os.path.join(empty_dir, "data"), exist_ok=True)

            db = DirSQL(config=cfg_path)
            state = vars(db)
            assert state["root"] == os.path.join(empty_dir, "data")
            assert state["ignore"] == ["node_modules/**"]
            assert state["persist"] is True
            assert state["persist_path"] == os.path.join(empty_dir, "cache.db")
            assert len(state["tables"]) == 1
            assert state["tables"][0] == {
                "ddl": "CREATE TABLE items (_path TEXT)",
                "glob": "*.json",
                "strict": True,
            }
