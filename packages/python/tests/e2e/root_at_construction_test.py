"""SDK e2e: the index root is read when ``DirSQL()`` is called.

Runs a real Python process that constructs ``DirSQL`` and changes directory
before the first await. No mocks: real interpreter, real binding, real
filesystem.
"""

import json
import subprocess
import sys

_DEFAULT_ROOT = """
import asyncio, json, os
from dirsql import DirSQL

async def main():
    os.chdir("B")
    db = DirSQL(no_ignore=True)
    os.chdir(os.path.join("..", "A"))
    rows = await db.query("SELECT content ->> 'who' AS w FROM './run.json'")
    print(json.dumps(rows))

asyncio.run(main())
"""

_RELATIVE_ROOT = """
import asyncio, json, os
from dirsql import DirSQL

async def main():
    db = DirSQL("tree")
    os.chdir(os.path.join("tree", "sub"))
    rows = await db.query("SELECT path FROM './**'")
    print(json.dumps(rows))

asyncio.run(main())
"""


def _run(script, cwd):
    result = subprocess.run(
        [sys.executable, "-c", script],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


def describe_root_at_construction():
    def it_roots_a_default_instance_at_the_cwd_of_construction(tmp_path):
        for who in ("A", "B"):
            (tmp_path / who).mkdir()
            (tmp_path / who / "run.json").write_text(
                json.dumps({"who": who}), encoding="utf-8"
            )

        assert _run(_DEFAULT_ROOT, tmp_path) == [{"w": "B"}]

    def it_resolves_a_relative_root_against_the_cwd_of_construction(tmp_path):
        (tmp_path / "tree" / "sub").mkdir(parents=True)
        (tmp_path / "tree" / "a.json").write_text("{}\n", encoding="utf-8")

        assert _run(_RELATIVE_ROOT, tmp_path) == [{"path": "a.json"}]
