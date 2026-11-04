"""Binding-tier tests for text SQLite holds as invalid UTF-8.

The query fails with an ordinary exception naming the column, and the
instance stays usable afterwards.
"""

import pytest

from dirsql import DirSQL

MESSAGE = 'column "v" holds text that is not valid UTF-8'


@pytest.fixture
def root(tmp_path):
    (tmp_path / "a.json").write_text("{}", encoding="utf-8")
    return str(tmp_path)


def describe_text_not_utf8():
    @pytest.mark.asyncio
    async def it_raises_an_exception_naming_the_column(root):
        db = DirSQL(root)
        with pytest.raises(Exception, match=MESSAGE):
            await db.query("SELECT CAST(x'ff41' AS TEXT) AS v")

    @pytest.mark.asyncio
    async def it_leaves_the_instance_usable(root):
        db = DirSQL(root)
        with pytest.raises(Exception, match=MESSAGE):
            await db.query("SELECT CAST(x'ff' AS TEXT) AS v FROM './a.json'")
        rows = await db.query("SELECT basename FROM './a.json'")
        assert rows == [{"basename": "a.json"}]
