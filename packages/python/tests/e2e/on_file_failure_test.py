"""SDK e2e: a per-file ``on_file`` failure fails ``ready()`` and the query with
an error naming the file, the table and the original error, whether the hook
raised or the table rejected its row.

Runs a real Python process over a real temp tree. No mocks: real interpreter,
real binding, real filesystem.
"""

import subprocess
import sys

import pytest

_SCRIPT = """
import asyncio, json
from dirsql import DirSQL, Table

def rows(path):
    with open(path) as f:
        return [{"id": path, "n": json.load(f)["n"]}]

async def main():
    db = DirSQL(".", tables=[Table(
        name="notes",
        ddl="CREATE TABLE notes (id TEXT PRIMARY KEY, n INTEGER NOT NULL)",
        glob="d/*.json",
        on_file=rows,
    )])
    for call in (db.ready, lambda: db.query("SELECT count(*) AS c FROM notes")):
        try:
            print(await call())
        except RuntimeError as e:
            print("ERROR", e)

asyncio.run(main())
"""


_CHANGED_SCRIPT = """
import asyncio, json
from dirsql import DirSQL, Table

def rows(path):
    with open(path) as f:
        return [{"id": path, "n": json.load(f)["n"]}]

def write(text):
    with open("d/a.json", "w") as f:
        f.write(text)

async def answer(db):
    try:
        return str(await db.query("SELECT n FROM notes"))
    except RuntimeError as e:
        return f"ERROR {e}"

async def query(db):
    print(await answer(db))

async def settle(db, expected):
    for _ in range(1000):
        if expected in await answer(db):
            return
        await asyncio.sleep(0.01)

async def main():
    db = DirSQL(".", tables=[Table(
        name="notes",
        ddl="CREATE TABLE notes (id TEXT PRIMARY KEY, n INTEGER NOT NULL)",
        glob="d/*.json",
        on_file=rows,
    )])
    await query(db)
    write('{"m": 2}')
    await settle(db, "KeyError")
    await query(db)
    await query(db)
    write('{"n": 5}')
    await settle(db, "5")
    await query(db)

asyncio.run(main())
"""


def _run(cwd, script=_SCRIPT):
    result = subprocess.run(
        [sys.executable, "-c", script],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    return result.stdout.splitlines()


def _tree(tmp_path, b):
    (tmp_path / "d").mkdir()
    (tmp_path / "d" / "a.json").write_text('{"n": 1}')
    (tmp_path / "d" / "b.json").write_text(b)
    (tmp_path / "d" / "c.json").write_text('{"n": 3}')


def _assert_fails(lines, *parts):
    assert len(lines) == 2, lines
    for line in lines:
        assert line.startswith("ERROR"), lines
        for part in parts:
            assert part in line, lines


def describe_on_file_failure():
    def it_fails_when_the_hook_raises(tmp_path):
        _tree(tmp_path, '{"m": 2}')
        _assert_fails(_run(tmp_path), "d/b.json", "notes", "KeyError")

    def it_fails_when_the_table_rejects_the_row(tmp_path):
        _tree(tmp_path, '{"n": null}')
        _assert_fails(_run(tmp_path), "d/b.json", "notes", "NOT NULL")

    @pytest.mark.skipif(sys.platform != "linux", reason="Linux only")
    def it_fails_every_query_while_a_changed_file_fails(tmp_path):
        (tmp_path / "d").mkdir()
        (tmp_path / "d" / "a.json").write_text('{"n": 1}')
        lines = _run(tmp_path, _CHANGED_SCRIPT)
        assert len(lines) == 4, lines
        assert lines[0] == "[{'n': 1}]", lines
        _assert_fails(lines[1:3], "d/a.json", "notes", "KeyError")
        assert lines[3] == "[{'n': 5}]", lines
