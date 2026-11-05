"""Binding-tier tests for value fidelity at the Python<->core boundary (#465).

Covers the numeric contract (out-of-i64 ints raise, no lossy Real/TEXT
fallback) and bytes mapping to BLOB.
"""

import os

import pytest

from dirsql import DirSQL, Table


def _db(tmp_dir, on_file, name="t", ddl="CREATE TABLE t (v)"):
    with open(os.path.join(tmp_dir, "marker.json"), "w") as f:
        f.write("{}")
    return DirSQL(
        tmp_dir, tables=[Table(name=name, ddl=ddl, glob="*.json", on_file=on_file)]
    )


def describe_integer_range():
    @pytest.mark.asyncio
    async def it_raises_on_int_exceeding_i64_max(tmp_dir):
        db = _db(tmp_dir, lambda path: [{"v": 2**63}])
        with pytest.raises(RuntimeError, match=str(2**63)):
            await db.ready()

    @pytest.mark.asyncio
    async def it_raises_on_int_below_i64_min(tmp_dir):
        db = _db(tmp_dir, lambda path: [{"v": -(2**63) - 1}])
        with pytest.raises(RuntimeError, match=str(-(2**63) - 1)):
            await db.ready()

    @pytest.mark.asyncio
    async def it_round_trips_i64_max(tmp_dir):
        db = _db(tmp_dir, lambda path: [{"v": 2**63 - 1}])
        await db.ready()
        rows = await db.query("SELECT v FROM t")
        assert rows[0]["v"] == 2**63 - 1


def describe_bytes():
    @pytest.mark.asyncio
    async def it_maps_bytes_to_blob(tmp_dir):
        db = _db(tmp_dir, lambda path: [{"v": b"\x01\x02\x03"}])
        await db.ready()
        rows = await db.query("SELECT v FROM t")
        assert rows[0]["v"] == b"\x01\x02\x03"
