"""SDK e2e: a relative ``root`` stays the directory it named at build time.

Runs a real Python process that builds ``DirSQL("tree")``, queries a
path-table, changes directory, and queries again. No mocks: real
interpreter, real binding, real filesystem.
"""

import json
import subprocess
import sys

_SCRIPT = """
import asyncio, json, os
from dirsql import DirSQL

async def main():
    db = DirSQL("tree")
    before = await db.query("SELECT path FROM './**'")
    os.chdir(os.path.join("tree", "sub"))
    after = await db.query("SELECT path FROM './**'")
    print(json.dumps({"before": before, "after": after}))

asyncio.run(main())
"""


def describe_relative_root_chdir():
    def it_returns_the_same_rows_before_and_after_a_chdir(tmp_path):
        (tmp_path / "tree" / "sub").mkdir(parents=True)
        (tmp_path / "tree" / "a.json").write_text("{}\n", encoding="utf-8")

        result = subprocess.run(
            [sys.executable, "-c", _SCRIPT],
            cwd=tmp_path,
            capture_output=True,
            text=True,
            check=False,
        )

        assert result.returncode == 0, result.stderr
        assert json.loads(result.stdout) == {
            "before": [{"path": "a.json"}],
            "after": [{"path": "a.json"}],
        }
