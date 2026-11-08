"""Binding-tier tests: ``db.lock`` holds the index still during a write.

The #1673 reproduction, with the writer wrapping each parent-then-child write
in ``db.lock``. Without the lock a query's read can see the child but not its
parent and fail on the foreign key.
"""

import asyncio
import json
import threading
import time
from pathlib import Path

import pytest

from dirsql import DirSQL, Table

PADS = 50
SECONDS = 10

SCHEMA = """
CREATE TABLE windows (
  id       TEXT PRIMARY KEY,            -- 'windows/W'
  position INTEGER NOT NULL UNIQUE      -- W, for ordering
);

CREATE TABLE forks (
  id        TEXT PRIMARY KEY,           -- 'windows/W/forks/F'
  window_id TEXT NOT NULL REFERENCES windows(id),
  position  INTEGER NOT NULL,           -- F
  UNIQUE (window_id, position)
);

CREATE TABLE turns (
  id       TEXT PRIMARY KEY,            -- 'windows/W/forks/F/turns/N'
  fork_id  TEXT NOT NULL REFERENCES forks(id),
  position INTEGER NOT NULL,            -- N
  UNIQUE (fork_id, position)
);

CREATE TABLE advices (
  id         TEXT PRIMARY KEY,          -- 'advices/<sha256>'
  text       TEXT NOT NULL UNIQUE,
  score      NUMERIC,          -- chars, or total bits
  score_text TEXT                       -- per-token bits rendering, NULL in chars mode
);

CREATE TABLE turn_advice (
  turn_id   TEXT NOT NULL UNIQUE REFERENCES turns(id),
  advice_id TEXT NOT NULL REFERENCES advices(id)
);

CREATE TABLE ts (
  id        TEXT PRIMARY KEY,           -- 'windows/W/forks/F/turns/N/ts/S'
  turn_id   TEXT NOT NULL REFERENCES turns(id),
  position  INTEGER NOT NULL,           -- S
  UNIQUE (turn_id, position)
);

CREATE TABLE t_attempts (
  id        TEXT PRIMARY KEY,           -- 'windows/W/forks/F/turns/N/ts/S/attempts/A'
  turn_id   TEXT NOT NULL REFERENCES turns(id),
  sample    INTEGER NOT NULL,           -- S
  position  INTEGER NOT NULL,           -- A
  exit_code INTEGER,                    -- NULL when timed out
  timed_out INTEGER NOT NULL,
  UNIQUE (turn_id, sample, position)
);

CREATE TABLE files (
  id         TEXT PRIMARY KEY,          -- 'windows/W/forks/F/turns/N/ts/S/attempts/A/src/<path>'
  attempt_id TEXT NOT NULL REFERENCES t_attempts(id),
  path       TEXT NOT NULL,             -- relative to the attempt's src/
  content    TEXT NOT NULL,
  UNIQUE (attempt_id, path)
);

CREATE TABLE test_results (
  id     TEXT PRIMARY KEY,              -- 'windows/W/forks/F/turns/N/ts/S/attempts/A/tests.json#<test>'
  t_id   TEXT NOT NULL REFERENCES ts(id),
  test   INTEGER NOT NULL,              -- test case number in the suite
  stdout TEXT NOT NULL,
  stderr TEXT NOT NULL,
  passed INTEGER NOT NULL,
  status TEXT NOT NULL,
  UNIQUE (t_id, test)
);

CREATE VIEW evaluated_turns AS
WITH tested AS (
  SELECT ts.turn_id, ts.position, coalesce(min(test_results.passed), 0) AS passed
  FROM ts JOIN test_results ON test_results.t_id = ts.id GROUP BY ts.id
)
SELECT turns.id, windows.position AS window, forks.position AS fork, turns.position AS turn,
       advices.text, advices.score, advices.score_text, count(*) AS samples, min(tested.passed) AS passed
FROM windows JOIN forks ON forks.window_id = windows.id
JOIN turns ON turns.fork_id = forks.id
JOIN turn_advice ON turn_advice.turn_id = turns.id
JOIN advices ON advices.id = turn_advice.advice_id
JOIN tested ON tested.turn_id = turns.id
WHERE advices.score IS NOT NULL
GROUP BY turns.id;
"""

TURN = "windows/*/forks/*/turns/*"


class _Database:
    def __init__(self, root, tables):
        self._query_lock = asyncio.Lock()
        self.dirsql = DirSQL(str(root), no_ignore=True, tables=tables)

    async def query(self, query, **params):
        async with self._query_lock:
            return await self.dirsql.query(query, params)


def _experiment_database(root):
    # Ids use forward slashes on every platform, as the advice paths do.
    def relative(path):
        return Path(path).relative_to(root)

    def window(path):
        window = relative(path).parent
        return [{"id": window.as_posix(), "position": int(window.name)}]

    def fork(path):
        fork = relative(path).parent
        return [{"id": fork.as_posix(), "window_id": fork.parent.parent.as_posix(), "position": int(fork.name)}]

    def turn(path):
        turn = relative(path).parent
        return [{"id": turn.as_posix(), "fork_id": turn.parent.parent.as_posix(), "position": int(turn.name)}]

    def advice(path):
        directory = relative(path).parent
        metadata = json.loads(Path(path).read_text())
        tokens = metadata.get("tokens")
        return [{
            "id": directory.as_posix(),
            "text": (Path(path).parent / "advice.txt").read_bytes().decode("utf-8"), "score": metadata.get("score"),
            "score_text": tokens and "".join(f"{token}<{bits:.1f}>" for token, bits in tokens),
        }]

    def turn_advice(path):
        metadata = json.loads(Path(path).read_text())
        if metadata["advice"] is None:
            return []
        return [{"turn_id": relative(path).parent.as_posix(), "advice_id": metadata["advice"]}]

    def generation(path):
        if json.loads(Path(path).read_text()).get("exit_code", -1) == -1:
            return []
        t = relative(path).parent.parent.parent
        return [{"id": t.as_posix(), "turn_id": t.parent.parent.as_posix(), "position": int(t.name)}]

    def t_attempt(path):
        attempt = relative(path).parent
        t = attempt.parent.parent
        generation = json.loads(Path(path).read_text())
        return [{
            "id": attempt.as_posix(), "turn_id": t.parent.parent.as_posix(), "sample": int(t.name),
            "position": int(attempt.name), "exit_code": generation.get("exit_code"),
            "timed_out": generation.get("timed_out", False),
        }]

    def test_results(path):
        tests = relative(path)
        return [{
            "id": f"{tests.as_posix()}#{case['index']}", "t_id": tests.parent.parent.parent.as_posix(), "test": case["index"],
            "stdout": case["stdout"], "stderr": case["stderr"], "passed": case["passed"], "status": case["status"],
        } for case in json.loads(Path(path).read_text())["cases"]]

    sources = {
        "windows": ("windows/*/window.json", window),
        "forks": ("windows/*/forks/*/fork.json", fork),
        "turns": (f"{TURN}/metadata.json", turn),
        "advices": ("advices/*/advice.json", advice),
        "turn_advice": (f"{TURN}/metadata.json", turn_advice),
        "ts": (f"{TURN}/ts/*/attempts/*/generation.json", generation),
        "t_attempts": (f"{TURN}/ts/*/attempts/*/generation.json", t_attempt),
        "test_results": (f"{TURN}/ts/*/attempts/*/tests.json", test_results),
    }
    statements = [statement for statement in SCHEMA.split(";") if statement.strip()]
    return _Database(root, [
        Table(name=name, ddl=statement, glob=sources.get(name, ("", None))[0], on_file=sources.get(name, ("", None))[1])
        for statement in statements for name in [statement.split()[2]]
    ])


def _seed(root):
    for w in range(1):
        (root / f"windows/{w}").mkdir(parents=True)
        (root / f"windows/{w}/window.json").write_text("{}")
        for f in range(4):
            d = root / f"windows/{w}/forks/{f}"
            d.mkdir(parents=True)
            (d / "fork.json").write_text("{}")
            for i in range(PADS):
                (d / f"pad/{i}").mkdir(parents=True)
                (d / f"pad/{i}/x.txt").write_text("x")
    (root / "advices").mkdir()


def describe_lock():
    @pytest.mark.asyncio
    async def it_keeps_a_query_from_reading_a_child_without_its_parent(tmp_path):
        root = tmp_path
        _seed(root)
        db = _experiment_database(root)
        await db.dirsql.ready()
        stop = threading.Event()
        failures = []

        def writer():
            n = 0
            try:
                while not stop.is_set():
                    with db.dirsql.lock:
                        a = root / f"advices/{n:064x}"
                        a.mkdir()
                        (a / "advice.txt").write_text(str(n))
                        (a / "advice.json").write_text("{}")
                        t = root / f"windows/0/forks/0/turns/{n}"
                        t.mkdir(parents=True)
                        (t / "metadata.json").write_text(
                            json.dumps({"advice": f"advices/{n:064x}"})
                        )
                    n += 1
                    time.sleep(0.003)
            except Exception as exc:
                failures.append(exc)

        thread = threading.Thread(target=writer)
        thread.start()
        errors = []
        queries = 0
        end = time.time() + SECONDS
        try:
            while time.time() < end and not failures:
                try:
                    await db.query("SELECT count(*) FROM turn_advice")
                    queries += 1
                except RuntimeError as exc:
                    errors.append(str(exc))
        finally:
            stop.set()
            thread.join()

        assert failures == []
        assert errors == []
        assert queries > 0
