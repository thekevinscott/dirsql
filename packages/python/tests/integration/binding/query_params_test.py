"""Binding-tier tests (real core) for bind parameters in ``query()``.

A sequence fills ``?`` placeholders and a mapping fills ``:name``
placeholders, through SQLite's own binding, for path-tables and declared
tables alike.
"""

import json

import pytest

from dirsql import DirSQL, Table

TRICKY = "it's \"quoted\"'; DROP TABLE notes; -- ünïcødé ✓"


@pytest.fixture
def json_dir(tmp_path):
    (tmp_path / "a.json").write_text(json.dumps({"advice": TRICKY}), encoding="utf-8")
    (tmp_path / "b.json").write_text(json.dumps({"advice": "plain"}), encoding="utf-8")
    return str(tmp_path)


def _read_note(path):
    with open(path, encoding="utf-8") as f:
        advice = json.load(f)["advice"]
    return [{"name": path.rsplit("/", 1)[-1].rsplit("\\", 1)[-1], "body": advice}]


def _notes(root):
    return DirSQL(
        root,
        tables=[
            Table(
                name="notes",
                ddl="CREATE TABLE notes (name TEXT, body TEXT)",
                glob="*.json",
                on_file=_read_note,
            )
        ],
    )


def describe_query_params():
    @pytest.mark.asyncio
    async def it_binds_a_named_parameter_for_a_path_table(json_dir):
        rows = await DirSQL(json_dir).query(
            "SELECT path FROM './*.json' WHERE content ->> 'advice' = :advice",
            {"advice": TRICKY},
        )

        assert rows == [{"path": "a.json"}]

    @pytest.mark.asyncio
    async def it_binds_a_sequence_for_a_declared_table(json_dir):
        rows = await _notes(json_dir).query(
            "SELECT name, body FROM notes WHERE body = ?", (TRICKY,)
        )

        assert rows == [{"name": "a.json", "body": TRICKY}]

    @pytest.mark.asyncio
    async def it_binds_each_native_type_as_itself(json_dir):
        rows = await DirSQL(json_dir).query(
            "SELECT typeof(?) AS i, typeof(?) AS r, typeof(?) AS t, "
            "typeof(?) AS b, typeof(?) AS n, ? AS flag, ? AS blob",
            [7, 1.5, "x", b"\x00\x01", None, True, bytearray(b"\x02")],
        )

        assert rows == [
            {
                "i": "integer",
                "r": "real",
                "t": "text",
                "b": "blob",
                "n": "null",
                "flag": 1,
                "blob": b"\x02",
            }
        ]

    @pytest.mark.asyncio
    async def it_still_runs_a_query_without_params(json_dir):
        rows = await _notes(json_dir).query("SELECT count(*) AS n FROM notes")

        assert rows == [{"n": 2}]

    @pytest.mark.asyncio
    async def it_rejects_a_value_sqlite_cannot_hold(json_dir):
        with pytest.raises(TypeError, match="unsupported parameter type: list"):
            await DirSQL(json_dir).query("SELECT ?", [[1, 2]])

    @pytest.mark.asyncio
    async def it_rejects_a_named_value_sqlite_cannot_hold(json_dir):
        with pytest.raises(TypeError, match="unsupported parameter type: object"):
            await DirSQL(json_dir).query("SELECT :a", {"a": object()})

    @pytest.mark.asyncio
    async def it_rejects_a_string_as_the_params(json_dir):
        with pytest.raises(TypeError, match="params must be a sequence or a mapping"):
            await DirSQL(json_dir).query("SELECT ?", "x")

    @pytest.mark.asyncio
    async def it_rejects_an_integer_out_of_range(json_dir):
        with pytest.raises(OverflowError):
            await DirSQL(json_dir).query("SELECT ?", [2**63])

    @pytest.mark.asyncio
    async def it_names_a_wrong_parameter_count(json_dir):
        with pytest.raises(
            RuntimeError,
            match=r"the query has 2 parameter\(s\), but 1 value\(s\) were given",
        ):
            await DirSQL(json_dir).query("SELECT ?, ?", [1])

    @pytest.mark.asyncio
    async def it_names_a_missing_named_parameter(json_dir):
        with pytest.raises(
            RuntimeError, match="the named parameter :advice has no value"
        ):
            await DirSQL(json_dir).query(
                "SELECT path FROM './*.json' WHERE content ->> 'advice' = :advice",
                {},
            )
