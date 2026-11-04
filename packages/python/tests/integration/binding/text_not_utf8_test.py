"""Binding-tier tests for text SQLite holds as invalid UTF-8.

The query returns it with replacement characters, as the CLI prints it, and
the instance stays usable afterwards.
"""

import pytest

from dirsql import DirSQL


@pytest.fixture
def root(tmp_path):
    (tmp_path / "a.json").write_text("{}", encoding="utf-8")
    return str(tmp_path)


def describe_text_not_utf8():
    @pytest.mark.asyncio
    async def it_returns_replacement_characters(root):
        db = DirSQL(root)
        rows = await db.query("SELECT CAST(x'ff41' AS TEXT) AS v")
        assert rows == [{"v": "�A"}]

    @pytest.mark.asyncio
    async def it_leaves_the_instance_usable(root):
        db = DirSQL(root)
        await db.query("SELECT CAST(x'ff' AS TEXT) AS v FROM './a.json'")
        rows = await db.query("SELECT basename FROM './a.json'")
        assert rows == [{"basename": "a.json"}]
