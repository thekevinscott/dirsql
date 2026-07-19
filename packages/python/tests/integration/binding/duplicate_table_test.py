"""Binding-tier tests (real core) for duplicate table-name registration.

Two table definitions sharing a name have no sane resolution -- last-one-wins
silently drops a table, and an opaque SQLite ``table already exists`` tells the
user nothing about *which* definitions collided. Registration fails instead,
naming the table and both sources so the user can find them.

The constructor only starts the background scan, so the failure surfaces from
``await db.ready()`` (the same shape as every other registration error).
"""

import pytest

from dirsql import DirSQL, Table


def describe_duplicate_table_names():
    @pytest.mark.asyncio
    async def it_rejects_two_programmatic_tables_sharing_a_name(tmp_path):
        db = DirSQL(
            str(tmp_path),
            tables=[
                Table(
                    ddl="CREATE TABLE dup (a TEXT)",
                    glob="**/*.a",
                    on_file=lambda path: [],
                ),
                Table(
                    ddl="CREATE TABLE dup (b TEXT)",
                    glob="**/*.b",
                    on_file=lambda path: [],
                ),
            ],
        )

        with pytest.raises(Exception) as excinfo:
            await db.ready()

        message = str(excinfo.value)
        assert "dup" in message
        assert "defined twice by a programmatic table" in message, message

    @pytest.mark.asyncio
    async def it_rejects_a_programmatic_table_colliding_with_a_config_table(tmp_path):
        config = tmp_path / "dirsql.toml"
        config.write_text(
            '[[table]]\nddl = "CREATE TABLE dup (a TEXT)"\nglob = "**/*.a"\n',
            encoding="utf-8",
        )

        db = DirSQL(
            str(tmp_path),
            tables=[
                Table(
                    ddl="CREATE TABLE dup (b TEXT)",
                    glob="**/*.b",
                    on_file=lambda path: [],
                )
            ],
            config=str(config),
        )

        with pytest.raises(Exception) as excinfo:
            await db.ready()

        message = str(excinfo.value)
        assert "dup" in message
        assert "programmatic table" in message, message
        assert str(config) in message, message
