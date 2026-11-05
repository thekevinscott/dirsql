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

async def main(value):
    db = DirSQL(".", tables=[Table(name="t", ddl="CREATE TABLE t (v)", glob="a.json",
        on_file=lambda p: [{"v": value}])])
    try:
        await db.ready()
    except RuntimeError as e:
        return str(e)
    return await db.query("SELECT v FROM t")

out = [asyncio.run(main(v)) for v in (decimal.Decimal(123456789012345678901), {"a": 1})]
print(json.dumps(out))
"""


def describe_on_file_value_types():
    def it_refuses_decimal_and_dict_values_with_a_type_error(tmp_path):
        (tmp_path / "a.json").write_text("{}\n", encoding="utf-8")

        result = subprocess.run(
            [sys.executable, "-c", _SCRIPT],
            cwd=tmp_path,
            capture_output=True,
            text=True,
            check=False,
        )

        assert result.returncode == 0, result.stderr
        wide, mapping = json.loads(result.stdout)
        assert "TypeError" in wide and "Decimal" in wide, wide
        assert "TypeError" in mapping and "dict" in mapping, mapping
