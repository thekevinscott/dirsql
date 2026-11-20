"""Binding-tier tests: a folder a path-table's glob reaches that exists but
cannot be listed fails the query with an error naming it."""

import os
import sys

import pytest

from dirsql import DirSQL

pytestmark = pytest.mark.skipif(
    sys.platform == "win32" or os.geteuid() == 0,
    reason="needs Unix permissions a non-root user cannot bypass",
)


@pytest.fixture
def locked(tmp_path):
    (tmp_path / "data" / "open").mkdir(parents=True)
    (tmp_path / "data" / "open" / "a.json").write_text("{}", encoding="utf-8")
    locked = tmp_path / "data" / "locked"
    locked.mkdir()
    (locked / "b.json").write_text("{}", encoding="utf-8")
    locked.chmod(0o311)
    yield locked
    locked.chmod(0o755)


def describe_path_table_over_an_unlistable_folder():
    @pytest.mark.asyncio
    async def it_raises_naming_the_folder(tmp_path, locked):
        db = DirSQL(str(tmp_path))
        with pytest.raises(Exception, match=f"cannot list folder '{locked}'"):
            await db.query("SELECT path FROM './data/**/*.json'")
