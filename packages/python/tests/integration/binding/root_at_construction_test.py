"""Binding-tier tests (real core): the index root is read when ``DirSQL()``
is called, so a ``chdir`` before the first await does not move it.
"""

import os

import pytest

from dirsql import DirSQL


@pytest.fixture
def cwd_restored(tmp_path):
    original = os.getcwd()
    os.chdir(tmp_path)
    try:
        yield tmp_path
    finally:
        os.chdir(original)


def describe_root_at_construction():
    @pytest.mark.asyncio
    async def it_roots_a_default_instance_at_the_cwd_of_construction(cwd_restored):
        for who in ("A", "B"):
            (cwd_restored / who).mkdir()
            (cwd_restored / who / f"{who}.json").write_text("{}\n", encoding="utf-8")
        os.chdir(cwd_restored / "B")
        db = DirSQL()
        os.chdir(cwd_restored / "A")

        rows = await db.query("SELECT path FROM './*.json'")

        assert rows == [{"path": "B.json"}]

    @pytest.mark.asyncio
    async def it_resolves_a_relative_root_against_the_cwd_of_construction(
        cwd_restored,
    ):
        (cwd_restored / "tree" / "sub").mkdir(parents=True)
        (cwd_restored / "tree" / "a.json").write_text("{}\n", encoding="utf-8")
        db = DirSQL("tree")
        os.chdir(cwd_restored / "tree" / "sub")

        rows = await db.query("SELECT path FROM './**'")

        assert rows == [{"path": "a.json"}]
