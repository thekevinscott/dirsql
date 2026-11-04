"""Binding-tier tests (real core): a relative ``root`` names the directory it
named when the instance was built, so a later ``chdir`` does not move it.
"""

import os

import pytest

from dirsql import DirSQL


@pytest.fixture
def relroot(tmp_path):
    (tmp_path / "tree" / "sub").mkdir(parents=True)
    (tmp_path / "tree" / "a.json").write_text("{}\n", encoding="utf-8")
    original = os.getcwd()
    os.chdir(tmp_path)
    try:
        yield tmp_path
    finally:
        os.chdir(original)


def describe_relative_root():
    @pytest.mark.asyncio
    async def it_keeps_path_tables_on_the_build_time_root_after_a_chdir(relroot):
        db = DirSQL("tree")
        os.chdir(relroot / "tree" / "sub")

        rows = await db.query("SELECT path FROM './**'")

        assert rows == [{"path": "a.json"}]

    @pytest.mark.asyncio
    async def it_does_not_read_a_same_named_directory_under_the_new_cwd(relroot):
        (relroot / "tree" / "sub" / "tree").mkdir()
        (relroot / "tree" / "sub" / "tree" / "other.json").write_text(
            "{}\n", encoding="utf-8"
        )
        db = DirSQL("tree")
        os.chdir(relroot / "tree" / "sub")

        rows = await db.query("SELECT path FROM './*.json'")

        assert rows == [{"path": "a.json"}]
