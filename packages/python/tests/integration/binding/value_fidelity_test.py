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
    # Since dirsql#714 an unrepresentable value is the hook's mistake and costs
    # its own file rather than the scan, so these assert the row never lands
    # instead of asserting a raise. The core still refuses the value and still
    # names it -- but that message rides on `scan_failures`, which this binding
    # cannot reach yet (#715). Until it can, "the value is named" is untestable
    # from Python, so `it_names_the_offending_value` has no assertion left to
    # make and is folded into the two range cases.
    @pytest.mark.asyncio
    async def it_skips_the_file_on_int_exceeding_i64_max(tmp_dir):
        db = _db(tmp_dir, lambda path: [{"v": 2**63}])
        await db.ready()
        assert await db.query("SELECT v FROM t") == []

    @pytest.mark.asyncio
    async def it_skips_the_file_on_int_below_i64_min(tmp_dir):
        db = _db(tmp_dir, lambda path: [{"v": -(2**63) - 1}])
        await db.ready()
        assert await db.query("SELECT v FROM t") == []

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
