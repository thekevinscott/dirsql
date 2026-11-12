"""CLI e2e: a writer creating a parent and then its child never fails a query
to a running `dirsql server`, and the server ends with every pair written.

The experiment layout: every 3 ms the writer creates `advices/N/advice.txt`
and `advices/N/advice.json`, then `windows/0/forks/0/turns/N/metadata.json`
naming that advice, beside four forks of padding directories, while the main
thread queries for 10 seconds. Linux only: there a query applies every event
the kernel has queued before it checks a foreign key. No mocks: real console
script, real server process, real sockets, real filesystem, real SQLite, real
`on-file` spawn.
"""

from __future__ import annotations

import json
import shutil
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

import pytest

pytestmark = pytest.mark.skipif(
    sys.platform != "linux", reason="the empty-queue guarantee is Linux's"
)

PADS = 50
SECONDS = 10
TURN = "windows/*/forks/*/turns/*"

HOOK = """\
import json, os, sys

root, table = sys.argv[1], sys.argv[2]
for path in sys.argv[3:]:
    directory = os.path.relpath(os.path.dirname(path), root)
    parts = directory.split(os.sep)
    rel = "/".join(parts)
    if table == "windows":
        row = {"id": rel, "position": int(parts[-1])}
    elif table == "advices":
        with open(os.path.join(os.path.dirname(path), "advice.txt")) as f:
            row = {"id": rel, "text": f.read()}
    elif table == "turn_advice":
        with open(path) as f:
            row = {"turn_id": rel, "advice_id": json.load(f)["advice"]}
    else:
        row = {
            "id": rel,
            "parent_id": "/".join(parts[:-2]),
            "position": int(parts[-1]),
        }
    print(json.dumps(row))
"""

TABLES = [
    (
        "windows",
        "CREATE TABLE windows (id TEXT PRIMARY KEY, position INTEGER NOT NULL)",
        "windows/*/window.json",
    ),
    (
        "forks",
        (
            "CREATE TABLE forks (id TEXT PRIMARY KEY,"
            " parent_id TEXT NOT NULL REFERENCES windows(id),"
            " position INTEGER NOT NULL)"
        ),
        "windows/*/forks/*/fork.json",
    ),
    (
        "turns",
        (
            "CREATE TABLE turns (id TEXT PRIMARY KEY,"
            " parent_id TEXT NOT NULL REFERENCES forks(id),"
            " position INTEGER NOT NULL)"
        ),
        f"{TURN}/metadata.json",
    ),
    (
        "advices",
        "CREATE TABLE advices (id TEXT PRIMARY KEY, text TEXT NOT NULL UNIQUE)",
        "advices/*/advice.json",
    ),
    (
        "turn_advice",
        (
            "CREATE TABLE turn_advice ("
            "turn_id TEXT NOT NULL UNIQUE REFERENCES turns(id),"
            " advice_id TEXT NOT NULL REFERENCES advices(id))"
        ),
        f"{TURN}/metadata.json",
    ),
]


def _cli() -> str:
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return dirsql


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _ask(port, sql):
    """The rows, or the error body's message when the query fails."""
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/query",
        data=json.dumps({"sql": sql}).encode(),
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as err:
        return json.loads(err.read())["error"]


def _config(hook):
    return "\n".join(
        f'[[table]]\nname = "{name}"\nddl = "{ddl}"\nglob = "{glob}"\n'
        f"on-file = \"'{sys.executable}' '{hook}' {{root}} {name}\"\n"
        for name, ddl, glob in TABLES
    )


def _seed(root):
    (root / "windows/0").mkdir(parents=True)
    (root / "windows/0/window.json").write_text("{}")
    for f in range(4):
        fork = root / f"windows/0/forks/{f}"
        fork.mkdir(parents=True)
        (fork / "fork.json").write_text("{}")
        for i in range(PADS):
            (fork / f"pad/{i}").mkdir(parents=True)
            (fork / f"pad/{i}/x.txt").write_text("x")
    (root / "advices").mkdir()


def _write_pair(root, n):
    advice = root / f"advices/{n:064x}"
    advice.mkdir()
    (advice / "advice.txt").write_text(str(n))
    (advice / "advice.json").write_text("{}")
    turn = root / f"windows/0/forks/0/turns/{n}"
    turn.mkdir(parents=True)
    (turn / "metadata.json").write_text(json.dumps({"advice": f"advices/{n:064x}"}))


def describe_a_server_while_a_writer_creates_parents_then_children():
    @pytest.fixture
    def served(tmp_path):
        root = tmp_path / "experiment"
        root.mkdir()
        _seed(root)
        hook = tmp_path / "rows.py"
        hook.write_text(HOOK)
        (root / ".dirsql.toml").write_text(_config(hook))
        port = _free_port()
        server = subprocess.Popen(
            [_cli(), "server", "-c", ".dirsql.toml", "--port", str(port)],
            cwd=str(root),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            ready = server.stdout.readline()
            assert ready.startswith("Running at"), ready + server.stderr.read()
            yield root, port
        finally:
            server.terminate()
            server.wait()

    def it_fails_no_query_and_ends_with_every_pair(served):
        root, port = served
        stop = threading.Event()
        written = []

        def writer():
            n = 0
            while not stop.is_set():
                _write_pair(root, n)
                n += 1
                written.append(n)
                time.sleep(0.003)

        thread = threading.Thread(target=writer)
        thread.start()
        failures = []
        queries = 0
        end = time.time() + SECONDS
        try:
            while time.time() < end:
                queries += 1
                answer = _ask(port, "SELECT count(*) AS n FROM turn_advice")
                if isinstance(answer, str):
                    failures.append(answer)
        finally:
            stop.set()
            thread.join()

        assert failures == [], (
            f"{len(failures)} of {queries} queries failed; first: {failures[0]}"
        )
        assert queries > 1
        pairs = written[-1]
        assert pairs > 100
        assert _ask(port, "SELECT count(*) AS n FROM turn_advice") == [{"n": pairs}]
