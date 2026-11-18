"""Binding tier: a relative ``extensions`` path is read when ``DirSQL()`` is
called, so a ``chdir`` before the first await does not move it.

The loadable is the repo's ``tests/fixtures/testext`` cdylib (registers
``dirsql_testext_answer() -> 42``), built on the fly with cargo.
"""

import os
import shutil

import pytest

from dirsql import DirSQL

from .extension_package_test import _build_fixture_extension


@pytest.fixture
def built(tmp_path):
    so = _build_fixture_extension(str(tmp_path / "target"))
    for who in ("A", "B"):
        (tmp_path / who).mkdir()
    original = os.getcwd()
    os.chdir(tmp_path / "A")
    try:
        yield tmp_path, so
    finally:
        os.chdir(original)


def describe_extensions_at_construction():
    @pytest.mark.asyncio
    async def it_resolves_a_relative_path_against_the_cwd_of_construction(built):
        tmp_path, so = built
        (tmp_path / "A" / "ext").mkdir()
        shutil.copy(so, tmp_path / "A" / "ext" / "testext.so")
        db = DirSQL(
            str(tmp_path / "A"),
            extensions=[
                {"path": "ext/testext.so", "entrypoint": "sqlite3_extension_init"}
            ],
        )
        os.chdir(tmp_path / "B")

        rows = await db.query("SELECT dirsql_testext_answer() AS a")

        assert rows == [{"a": 42}]

    @pytest.mark.asyncio
    async def it_probes_a_bare_name_shadow_in_the_cwd_of_construction(built):
        tmp_path, so = built
        shutil.copy(so, tmp_path / "A" / "testext")
        db = DirSQL(
            str(tmp_path / "A"),
            extensions=[{"path": "testext", "entrypoint": "sqlite3_extension_init"}],
        )
        os.chdir(tmp_path / "B")

        rows = await db.query("SELECT dirsql_testext_answer() AS a")

        assert rows == [{"a": 42}]
