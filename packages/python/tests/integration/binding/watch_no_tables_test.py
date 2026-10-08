"""Binding-tier test: watch() with no named tables fails loudly."""

import pytest

from dirsql import DirSQL


def describe_watch_without_named_tables():
    @pytest.mark.asyncio
    async def it_raises_naming_the_cause(tmp_dir):
        db = DirSQL(tmp_dir)
        await db.ready()
        with pytest.raises(Exception, match="path-tables emit no watch events"):
            async for _ in db.watch():
                break
