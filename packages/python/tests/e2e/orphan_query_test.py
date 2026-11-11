"""CLI e2e: while a child references no parent, every query to a running
`dirsql server` fails with the foreign-key error, until the parent is written
or the child deleted. Linux only: there a query applies every event the kernel
has queued first. No mocks: real console script, real server process, real
sockets, real filesystem, real SQLite, real `on-file` spawn.
"""

from __future__ import annotations

import json
import shutil
import socket
import subprocess
import sys
import urllib.error
import urllib.request

import pytest

pytestmark = pytest.mark.skipif(
    sys.platform != "linux", reason="the empty-queue guarantee is Linux's"
)

CONFIG = """\
[[table]]
name = "parent"
ddl = "CREATE TABLE parent (id INTEGER PRIMARY KEY)"
glob = "p/*.json"
on-file = "cat"

[[table]]
name = "child"
ddl = "CREATE TABLE child (id TEXT, pid INTEGER REFERENCES parent(id))"
glob = "c/*.json"
on-file = "cat"
"""

COUNTS = (
    "SELECT (SELECT count(*) FROM parent) AS parents,"
    " (SELECT count(*) FROM child) AS children"
)


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
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as err:
        return json.loads(err.read())["error"]


def describe_a_server_over_a_child_with_no_parent():
    @pytest.fixture
    def served(tmp_path):
        root = tmp_path / "orphan-root"
        (root / "p").mkdir(parents=True)
        (root / "c").mkdir()
        (root / ".dirsql.toml").write_text(CONFIG)
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
            (root / "c" / "lost.json").write_text('{"id": "lost", "pid": 1}\n')
            yield root, port
        finally:
            server.terminate()
            server.wait(timeout=60)

    def _assert_fails(port):
        answer = _ask(port, COUNTS)
        assert isinstance(answer, str), answer
        assert "foreign key violation" in answer, answer
        assert "c/lost.json" in answer, answer

    def it_fails_every_query_until_the_parent_is_written(served):
        root, port = served
        _assert_fails(port)
        _assert_fails(port)

        (root / "p" / "one.json").write_text('{"id": 1}\n')

        assert _ask(port, COUNTS) == [{"parents": 1, "children": 1}]

    def it_clears_when_the_child_is_deleted(served):
        root, port = served
        _assert_fails(port)

        (root / "c" / "lost.json").unlink()

        assert _ask(port, COUNTS) == [{"parents": 0, "children": 0}]
