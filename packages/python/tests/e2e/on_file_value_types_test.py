"""SDK e2e: an ``on_file`` value of an unsupported type is refused, not
stored as a lossy REAL or a repr (docs/reference/sdk.md, "Value types").

Runs a real Python process whose hook returns a ``Decimal`` too wide for a
float and a ``dict``. No mocks: real interpreter, real binding, real
filesystem.
"""

import json
import subprocess
import sys

_SCRIPT = """
import asyncio, decimal, json
from dirsql import DirSQL, Table

VALUES = {"wide.json": decimal.Decimal("123456789012345678901"), "map.json": {"a": 1}}

async def main():
    db = DirSQL(".", tables=[Table(name="t", ddl="CREATE TABLE t (v)", glob="*.json",
        on_file=lambda p: [{"v": VALUES[p.rsplit("/", 1)[-1].rsplit(chr(92), 1)[-1]]}])])
    rows = await db.query("SELECT v FROM t")
    failures = sorted((f.path, f.message) for f in await db.scan_failures())
    print(json.dumps({"rows": rows, "failures": failures}))

asyncio.run(main())
"""


def describe_on_file_value_types():
    def it_refuses_decimal_and_dict_values_with_a_type_error(tmp_path):
        (tmp_path / "wide.json").write_text("{}\n", encoding="utf-8")
        (tmp_path / "map.json").write_text("{}\n", encoding="utf-8")

        result = subprocess.run(
            [sys.executable, "-c", _SCRIPT],
            cwd=tmp_path,
            capture_output=True,
            text=True,
            check=False,
        )

        assert result.returncode == 0, result.stderr
        out = json.loads(result.stdout)
        assert out["rows"] == []
        (map_path, map_msg), (wide_path, wide_msg) = out["failures"]
        assert map_path.endswith("map.json")
        assert "TypeError" in map_msg and "dict" in map_msg, map_msg
        assert wide_path.endswith("wide.json")
        assert "TypeError" in wide_msg and "Decimal" in wide_msg, wide_msg
