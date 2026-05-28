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
            await db.ready()

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
            await db.ready()

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
            await db.ready()

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
            await db.ready()

            state = vars(db)
            for t in state["tables"]:
                assert "name" not in t

        @pytest.mark.asyncio
        async def it_exposes_table_state_via_vars(empty_dir):
            """A standalone `Table` is also serializable via `vars()`."""
            t = Table(
                ddl="CREATE TABLE items (name TEXT)",
                glob="items/*.json",
                extract=_noop_extract,
                strict=True,
            )

            state = vars(t)
            assert isinstance(state, dict)
            assert state == {
                "ddl": "CREATE TABLE items (name TEXT)",
                "glob": "items/*.json",
                "strict": True,
            }

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
            await db.ready()
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
            await db.ready()
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
            await db.ready()
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
            await db.ready()
            state = vars(db)
            assert state["ignore"] == ["**/skip/**", "**/temp/**"]
