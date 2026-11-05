"""Binding-tier tests for the value types an ``on_file`` row may carry
(docs/reference/sdk.md, "Value types"): ``str``, ``int``/``bool``, ``float``,
``bytes``/``bytearray`` and ``None`` are stored; any other type is refused
with a ``TypeError`` rather than coerced to REAL or to its repr.
"""

import decimal
import fractions
import os

import pytest

from dirsql import DirSQL, Table


class _IntLike:
    """Stands in for a numpy integer: not an ``int``, but has ``__index__``."""

    def __index__(self):
        return 2**53 + 1

    def __float__(self):
        return float(2**53 + 1)


def _db(tmp_dir, value):
    with open(os.path.join(tmp_dir, "a.json"), "w") as f:
        f.write("{}")
    return DirSQL(
        tmp_dir,
        tables=[
            Table(
                name="t",
                ddl="CREATE TABLE t (v)",
                glob="a.json",
                on_file=lambda path: [{"v": value}],
            )
        ],
    )


_UNSUPPORTED = [
    (decimal.Decimal(123456789012345678901), "Decimal"),
    (fractions.Fraction(1, 3), "Fraction"),
    (_IntLike(), "_IntLike"),
    ({"a": 1, "b": None}, "dict"),
    ([1, 2, 3], "list"),
    (memoryview(b"ab"), "memoryview"),
]


def describe_on_file_value_types():
    @pytest.mark.asyncio
    @pytest.mark.parametrize(("value", "type_name"), _UNSUPPORTED)
    async def it_fails_the_build_on_an_unsupported_type(tmp_dir, value, type_name):
        db = _db(tmp_dir, value)

        with pytest.raises(RuntimeError) as raised:
            await db.ready()
        message = str(raised.value)
        assert "TypeError" in message, message
        assert type_name in message, message
        assert "'v'" in message, message

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            ("s", "s"),
            (7, 7),
            (True, 1),
            (0.5, 0.5),
            (b"\x00\x01", b"\x00\x01"),
            (bytearray(b"\x02"), b"\x02"),
            (None, None),
        ],
    )
    async def it_stores_each_supported_type_as_given(tmp_dir, value, expected):
        db = _db(tmp_dir, value)
        await db.ready()

        assert await db.query("SELECT v FROM t") == [{"v": expected}]
