"""Integration tests for structured column definitions (issue #202).

These exercise the new ``Table(name=..., columns=[...])`` shape that replaces
the raw ``ddl=`` string. Each test builds a real ``DirSQL`` over a temp
directory and inspects the resulting SQLite schema through the read-only
``query()`` API (``pragma_table_info``, ``pragma_index_list``,
``sqlite_master``). A passing test therefore proves the Rust core generated
the expected ``CREATE TABLE`` statement from the structured shape.

Written test-first (red/green): until the binding accepts ``name=`` /
``columns=`` and the core grows ``Table::to_ddl``, every test here is expected
to fail.
"""

import os

import pytest

import dirsql
from dirsql import DirSQL, Table


async def _cols(db, table):
    """Return ``{name: row}`` from ``pragma_table_info`` minus tracking cols."""
    rows = await db.query(
        'SELECT name, type, "notnull" AS nn, dflt_value, pk '
        f"FROM pragma_table_info('{table}')"
    )
    return {r["name"]: r for r in rows if not r["name"].startswith("_dirsql_")}


async def _table_sql(db, table):
    """Return the stored ``CREATE TABLE`` text for ``table``."""
    rows = await db.query(
        f"SELECT sql FROM sqlite_master WHERE type='table' AND name='{table}'"
    )
    return rows[0]["sql"]


async def _indexes(db, table):
    """Return ``{index_name: is_unique}`` for user-declared indexes."""
    rows = await db.query(
        f"SELECT name, \"unique\" AS uq FROM pragma_index_list('{table}')"
    )
    return {r["name"]: bool(r["uq"]) for r in rows}


def describe_structured_columns():
    def describe_basic():
        @pytest.mark.asyncio
        async def it_creates_table_from_columns(tmp_dir):
            """A table defined by name + columns round-trips data through query."""
            os.makedirs(os.path.join(tmp_dir, "docs"), exist_ok=True)
            with open(os.path.join(tmp_dir, "docs", "a.md"), "w") as f:
                f.write("# hi")

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="docs",
                        glob="docs/*.md",
                        columns=[
                            {"name": "title", "type": "TEXT"},
                            {"name": "body", "type": "TEXT"},
                        ],
                        extract=lambda path: [{"title": "hi", "body": "world"}],
                    ),
                ],
            )
            await db.ready()
            rows = await db.query("SELECT title, body FROM docs")
            assert rows == [{"title": "hi", "body": "world"}]

        @pytest.mark.asyncio
        async def it_supports_all_storage_types(tmp_dir):
            """Every SQLite storage class maps to the right column type."""
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.bin",
                        columns=[
                            {"name": "a", "type": "TEXT"},
                            {"name": "b", "type": "INTEGER"},
                            {"name": "c", "type": "REAL"},
                            {"name": "d", "type": "BLOB"},
                            {"name": "e", "type": "NUMERIC"},
                        ],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            cols = await _cols(db, "t")
            assert cols["a"]["type"] == "TEXT"
            assert cols["b"]["type"] == "INTEGER"
            assert cols["c"]["type"] == "REAL"
            assert cols["d"]["type"] == "BLOB"
            assert cols["e"]["type"] == "NUMERIC"

    def describe_column_constraints():
        @pytest.mark.asyncio
        async def it_marks_not_null(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[{"name": "title", "type": "TEXT", "not_null": True}],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            cols = await _cols(db, "t")
            assert cols["title"]["nn"] == 1

        @pytest.mark.asyncio
        async def it_marks_primary_key(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[{"name": "id", "type": "TEXT", "primary_key": True}],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            cols = await _cols(db, "t")
            assert cols["id"]["pk"] == 1

        @pytest.mark.asyncio
        async def it_marks_unique(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[{"name": "slug", "type": "TEXT", "unique": True}],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            assert "UNIQUE" in await _table_sql(db, "t")

        @pytest.mark.asyncio
        async def it_emits_autoincrement(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[
                            {
                                "name": "id",
                                "type": "INTEGER",
                                "primary_key": True,
                                "autoincrement": True,
                            }
                        ],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            assert "AUTOINCREMENT" in await _table_sql(db, "t")

        @pytest.mark.asyncio
        async def it_emits_collate(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[{"name": "name", "type": "TEXT", "collate": "NOCASE"}],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            assert "COLLATE NOCASE" in await _table_sql(db, "t")

        @pytest.mark.asyncio
        async def it_emits_scalar_default(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[
                            {
                                "name": "title",
                                "type": "TEXT",
                                "default": "untitled",
                            }
                        ],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            cols = await _cols(db, "t")
            assert cols["title"]["dflt_value"] == "'untitled'"

    def describe_escape_hatch():
        @pytest.mark.asyncio
        async def it_supports_sql_default_expression(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[
                            {
                                "name": "ingested_at",
                                "type": "INTEGER",
                                "default": {"sql": "strftime('%s', 'now')"},
                            }
                        ],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            cols = await _cols(db, "t")
            assert cols["ingested_at"]["dflt_value"] == "strftime('%s', 'now')"

        @pytest.mark.asyncio
        async def it_supports_check_constraint(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[
                            {
                                "name": "body",
                                "type": "TEXT",
                                "check": {"sql": "length(body) > 0"},
                            }
                        ],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            sql = await _table_sql(db, "t")
            assert "CHECK" in sql
            assert "length(body) > 0" in sql

        @pytest.mark.asyncio
        async def it_supports_generated_column(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[
                            {"name": "body", "type": "TEXT"},
                            {
                                "name": "body_len",
                                "type": "INTEGER",
                                "generated": {
                                    "sql": "length(body)",
                                    "mode": "stored",
                                },
                            },
                        ],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            sql = await _table_sql(db, "t")
            assert "length(body)" in sql
            assert "STORED" in sql.upper()

    def describe_table_level():
        @pytest.mark.asyncio
        async def it_supports_composite_primary_key(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[
                            {"name": "a", "type": "TEXT"},
                            {"name": "b", "type": "TEXT"},
                        ],
                        primary_key=["a", "b"],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            cols = await _cols(db, "t")
            assert cols["a"]["pk"] == 1
            assert cols["b"]["pk"] == 2

        @pytest.mark.asyncio
        async def it_supports_composite_unique(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[
                            {"name": "a", "type": "TEXT"},
                            {"name": "b", "type": "TEXT"},
                        ],
                        unique=[["a", "b"]],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            assert "UNIQUE" in await _table_sql(db, "t")

        @pytest.mark.asyncio
        async def it_supports_indexes(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[{"name": "title", "type": "TEXT"}],
                        indexes=[
                            {"name": "idx_title", "columns": ["title"], "unique": True}
                        ],
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            indexes = await _indexes(db, "t")
            assert indexes.get("idx_title") is True

        @pytest.mark.asyncio
        async def it_supports_without_rowid(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[{"name": "id", "type": "TEXT", "primary_key": True}],
                        without_rowid=True,
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            assert "WITHOUT ROWID" in (await _table_sql(db, "t")).upper()

        @pytest.mark.asyncio
        async def it_supports_strict_types(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        name="t",
                        glob="*.md",
                        columns=[{"name": "title", "type": "TEXT"}],
                        strict_types=True,
                        extract=lambda path: [],
                    ),
                ],
            )
            await db.ready()
            assert "STRICT" in (await _table_sql(db, "t")).upper()

    def describe_deprecation():
        def it_warns_on_ddl():
            """The legacy ``ddl=`` shape still works but emits a DeprecationWarning."""
            with pytest.warns(DeprecationWarning):
                Table(
                    ddl="CREATE TABLE t (x TEXT)",
                    glob="*.md",
                    extract=lambda path: [],
                )

        def it_errors_on_ddl_and_columns():
            """Supplying both ``ddl`` and ``columns`` is a hard error."""
            with pytest.raises(ValueError, match="(?i)ddl.*column|column.*ddl|both"):
                Table(
                    ddl="CREATE TABLE t (x TEXT)",
                    name="t",
                    glob="*.md",
                    columns=[{"name": "x", "type": "TEXT"}],
                    extract=lambda path: [],
                )

    def describe_type_constants():
        def it_exports_type_strings():
            """The five storage-type strings are exported for autocomplete."""
            assert dirsql.TEXT == "TEXT"
            assert dirsql.INTEGER == "INTEGER"
            assert dirsql.REAL == "REAL"
            assert dirsql.BLOB == "BLOB"
            assert dirsql.NUMERIC == "NUMERIC"
