"""Binding-tier test for ``scan_failures``: a scan that builds has no failures,
since any per-file ``on_file`` failure fails the build instead.
"""

import os

import pytest

from dirsql import DirSQL, Table


def describe_scan_failures():
    @pytest.mark.asyncio
    async def it_is_empty_after_a_clean_scan(tmp_dir):
        with open(os.path.join(tmp_dir, "a.json"), "w") as f:
            f.write("{}")
        db = DirSQL(
            tmp_dir,
            tables=[
                Table(
                    name="items",
                    ddl="CREATE TABLE items (name TEXT)",
                    glob="*.json",
                    on_file=lambda path: [{"name": "ok"}],
                )
            ],
        )
        await db.ready()
        assert await db.scan_failures() == []
