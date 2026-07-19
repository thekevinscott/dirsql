"""Binding-tier tests (real core) for duplicate table-name registration.

Two table definitions sharing a name have no sane resolution -- last-one-wins
silently drops a table, and an opaque SQLite ``table already exists`` tells the
user nothing about *which* definitions collided. Registration fails instead,
naming the table and both sources so the user can find them.
"""

import pytest

from dirsql import DirSQL, Table


def describe_duplicate_table_names():
    def it_rejects_two_programmatic_tables_sharing_a_name(tmp_path):
        with pytest.raises(Exception) as excinfo:
            DirSQL(
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

        message = str(excinfo.value)
        assert "dup" in message
        assert message.count("programmatic table") == 2, message

    def it_rejects_a_programmatic_table_colliding_with_a_config_table(tmp_path):
        config = tmp_path / "dirsql.toml"
        config.write_text(
            '[[table]]\nddl = "CREATE TABLE dup (a TEXT)"\nglob = "**/*.a"\n',
            encoding="utf-8",
        )

        with pytest.raises(Exception) as excinfo:
            DirSQL(
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

        message = str(excinfo.value)
        assert "dup" in message
        assert "programmatic table" in message, message
        assert str(config) in message, message
