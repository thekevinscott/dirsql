"""E2E: a writer creating a parent and then its child never fails a query.

The writer makes each advice and the turn referencing it in new directories
while the main thread queries in a loop. No mocks: real binding, real
filesystem, real SQLite.
"""

from __future__ import annotations

import asyncio
import sys
import threading
import time
from pathlib import Path

import pytest

from dirsql import DirSQL, Table

pytestmark = pytest.mark.skipif(sys.platform != "linux", reason="Linux only")


def _advice() -> Table:
    return Table(
        name="advice",
        ddl="CREATE TABLE advice (id TEXT PRIMARY KEY)",
        glob="advices/*/advice.txt",
        on_file=lambda path: [{"id": Path(path).parent.name}],
    )


def _turn() -> Table:
    def on_file(path):
        advice = Path(path).read_text()
        if not advice:
            raise ValueError("no advice yet")
        return [{"n": Path(path).parent.name, "advice": advice}]

    return Table(
        name="turn",
        ddl="CREATE TABLE turn (n TEXT, advice TEXT REFERENCES advice(id))",
        glob="turns/*/metadata.txt",
        on_file=on_file,
    )


def _write_pair(root: Path, n: int) -> None:
    advice = f"{n:064x}"
    (root / "advices" / advice).mkdir()
    (root / "advices" / advice / "advice.txt").write_text("x")
    (root / "turns" / str(n)).mkdir()
    (root / "turns" / str(n) / "metadata.txt").write_text(advice)


def describe_a_writer_creating_parents_then_children():
    @pytest.mark.asyncio
    async def it_fails_no_query(tmp_path):
        (tmp_path / "advices").mkdir()
        (tmp_path / "turns").mkdir()
        db = DirSQL(str(tmp_path), tables=[_advice(), _turn()])
        await db.ready()
        stop = threading.Event()

        def writer():
            n = 0
            while not stop.is_set():
                _write_pair(tmp_path, n)
                n += 1
                time.sleep(0.003)

        thread = threading.Thread(target=writer)
        thread.start()
        failures = []
        queries = 0
        end = time.time() + 5
        try:
            while time.time() < end:
                queries += 1
                try:
                    await db.query("SELECT count(*) AS n FROM turn")
                except Exception as e:
                    failures.append(str(e))
        finally:
            stop.set()
            thread.join()
        assert failures == [], f"{len(failures)} of {queries} queries failed"

    @pytest.mark.asyncio
    async def it_gets_each_pair_into_the_index(tmp_path):
        (tmp_path / "advices").mkdir()
        (tmp_path / "turns").mkdir()
        db = DirSQL(str(tmp_path), tables=[_advice(), _turn()])
        await db.ready()
        for n in range(100):
            _write_pair(tmp_path, n)
            rows = await db.query("SELECT count(*) AS n FROM turn")
            for _ in range(1000):
                if rows == [{"n": n + 1}]:
                    break
                await asyncio.sleep(0.01)
                rows = await db.query("SELECT count(*) AS n FROM turn")
            assert rows == [{"n": n + 1}], n
