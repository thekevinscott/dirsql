"""Binding-tier tests for the scan's record of files it skipped (#715).

Since #714 a file whose ``on_file`` hook raises, or whose row the table
rejects, is skipped rather than failing the scan. The CLI reports those skips
on stderr and exits 23; a Python caller had no equivalent, so an incomplete
index was indistinguishable from a complete one -- the regression this closes.
"""

import os

import pytest

from dirsql import DirSQL, Table


def _db(tmp_dir, on_file, ddl="CREATE TABLE items (name TEXT)", **kw):
    return DirSQL(
        tmp_dir,
        tables=[Table(ddl=ddl, glob="*.json", on_file=on_file, **kw)],
    )


def _write(tmp_dir, *names):
    for name in names:
        with open(os.path.join(tmp_dir, name), "w") as f:
            f.write("{}")


def describe_scan_failures():
    @pytest.mark.asyncio
    async def it_is_empty_after_a_clean_scan(tmp_dir):
        _write(tmp_dir, "a.json")
        db = _db(tmp_dir, lambda path: [{"name": "ok"}])
        await db.ready()
        assert await db.scan_failures() == []

    @pytest.mark.asyncio
    async def it_names_each_skipped_file(tmp_dir):
        _write(tmp_dir, "good.json", "bad.json")

        def on_file(path):
            if path.endswith("bad.json"):
                raise ValueError("boom")
            return [{"name": "ok"}]

        db = _db(tmp_dir, on_file)
        await db.ready()

        failures = await db.scan_failures()
        assert len(failures) == 1, failures
        assert failures[0].path.endswith("bad.json"), failures[0].path
        # The row that did land is untouched: this reports, it does not gate.
        assert await db.query("SELECT name FROM items") == [{"name": "ok"}]

    @pytest.mark.asyncio
    async def it_carries_the_hooks_own_message(tmp_dir):
        _write(tmp_dir, "a.json")
        db = _db(tmp_dir, lambda path: (_ for _ in ()).throw(ValueError("boom-xyzzy")))
        await db.ready()

        (failure,) = await db.scan_failures()
        assert "boom-xyzzy" in failure.message, failure.message

    @pytest.mark.asyncio
    async def it_reports_a_row_the_table_rejected(tmp_dir):
        # Not just hook exceptions: a strict-mode violation is the same kind of
        # per-file failure, and the message still names the offending column.
        _write(tmp_dir, "a.json")
        db = _db(tmp_dir, lambda path: [{"nope": 1}], strict=True)
        await db.ready()

        (failure,) = await db.scan_failures()
        assert failure.path.endswith("a.json"), failure.path
        assert "nope" in failure.message, failure.message

    @pytest.mark.asyncio
    async def it_reports_every_skipped_file_not_only_the_first(tmp_dir):
        _write(tmp_dir, "a.json", "b.json", "c.json")
        db = _db(tmp_dir, lambda path: (_ for _ in ()).throw(ValueError("boom")))
        await db.ready()

        reported = {os.path.basename(f.path) for f in await db.scan_failures()}
        assert reported == {"a.json", "b.json", "c.json"}, reported
