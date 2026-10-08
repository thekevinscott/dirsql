"""Server speed: repeated queries against a warm `dirsql server`.

A table of rows is indexed once from tab-separated files by an `on-file`
parser; the question is a filtered aggregate asked several times with a
different needle each time, so no answer can be reused. Native is the same
rows held in one SQLite connection on an anonymous disk-backed temp database,
the storage dirsql deliberately uses to bound memory, queried with the same
SQL in process with no HTTP hop. Only the queries are
timed. Indexing and server start are fixed costs paid once, so neither side
is charged for them. No mocks: real console script, real server process,
real sockets, real filesystem.
"""

from __future__ import annotations

import json
import shutil
import socket
import sqlite3
import subprocess
import time
import urllib.request

import pytest

from .speed_of_light import (
    ONE_SECOND,
    TOLERANCE,
    cli,
    grow_until_native_takes_a_second,
)

BEST_OF = 3

COLUMNS = ("k", "n", "bytes", "tail")
NEEDLES = ("1", "3", "5", "7", "9")
ROWS_PER_FILE = 20_000
GROUPS = 16
DDL = "CREATE TABLE rows (id INTEGER, k TEXT, v INTEGER, body TEXT)"

CONFIG = f"""\
[[table]]
name    = "rows"
glob    = "data/*.tsv"
ddl     = "{DDL}"
on-file = "python3 rows.py"
"""

ROWS = """\
import json, sys

for path in sys.argv[1:]:
    for line in open(path, encoding="utf-8"):
        i, k, v, body = line.rstrip("\\n").split("\\t")
        print(json.dumps({"id": int(i), "k": k, "v": int(v), "body": body}))
"""


def query(needle):
    return (
        "SELECT k, count(*) AS n, sum(length(body)) AS bytes,"
        " max(substr(body, 3, 6)) AS tail"
        f" FROM rows WHERE body LIKE '%{needle}%' GROUP BY k ORDER BY k"
    )


def row(i):
    return (i, f"k{i % GROUPS}", i * 7 % 1000, f"row {i} says {i * 2654435761 % 10**9}")


def build_corpus(root, db, lo, hi):
    """Rows `lo` through `hi - 1`, written to files and inserted natively."""
    if lo == 0:
        root.mkdir(parents=True)
        (root / "data").mkdir()
        (root / ".dirsql.toml").write_text(CONFIG)
        (root / "rows.py").write_text(ROWS)
    for start in range(lo, hi, ROWS_PER_FILE):
        stop = min(start + ROWS_PER_FILE, hi)
        batch = [row(i) for i in range(start, stop)]
        lines = ["\t".join(map(str, r)) for r in batch]
        (root / "data" / f"part-{start:09d}.tsv").write_text("\n".join(lines) + "\n")
        db.executemany("INSERT INTO rows VALUES (?, ?, ?, ?)", batch)
    db.commit()


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def post_query(port, sql):
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/query",
        data=json.dumps({"sql": sql}).encode(),
        headers={"content-type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=300) as response:
        return json.loads(response.read())


def run_all(ask):
    started = time.perf_counter()
    answers = [ask(query(needle)) for needle in NEEDLES]
    return answers, time.perf_counter() - started


def best(ask):
    fastest = None
    for _ in range(BEST_OF):
        answers, seconds = run_all(ask)
        if fastest is None or seconds < fastest[1]:
            fastest = (answers, seconds)
    return fastest


def native_ask(db):
    def ask(sql):
        return [dict(zip(COLUMNS, r)) for r in db.execute(sql).fetchall()]

    return ask


def describe_server_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "server-speed"
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    @pytest.fixture
    def db():
        conn = sqlite3.connect("")
        conn.execute(DDL)
        try:
            yield conn
        finally:
            conn.close()

    def it_answers_repeated_queries_within_the_bar(root, db):
        ceiling = 2**23
        ask_native = native_ask(db)
        size = grow_until_native_takes_a_second(
            lambda lo, hi: build_corpus(root, db, lo, hi),
            lambda: run_all(ask_native),
            start=ROWS_PER_FILE * 8,
            ceiling=ceiling,
        )
        assert size >= ROWS_PER_FILE
        expected, native_seconds = best(ask_native)
        assert native_seconds >= ONE_SECOND, native_seconds
        assert all(len(answer) == GROUPS for answer in expected)

        port = free_port()
        server = subprocess.Popen(
            [cli(), "server", "-c", ".dirsql.toml", "--port", str(port)],
            cwd=str(root),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            ready = server.stdout.readline()
            assert ready.startswith("Running at"), ready + server.stderr.read()
            warm = post_query(port, "SELECT count(*) AS n FROM rows")
            assert warm == [{"n": size}]

            actual, dirsql_seconds = best(lambda sql: post_query(port, sql))
        finally:
            server.terminate()
            server.wait(timeout=60)

        assert actual == expected
        budget = TOLERANCE * native_seconds
        assert dirsql_seconds <= budget, (
            f"server: {len(NEEDLES)} queries took {dirsql_seconds:.3f}s,"
            f" over {TOLERANCE}x native {native_seconds:.3f}s = {budget:.3f}s"
        )
