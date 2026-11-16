"""E2E: a directory the server's watcher cannot watch fails every query.

Linux only: a directory without read permission is one inotify cannot watch.
No mocks: real console script, real server process, real sockets, real
filesystem.
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

import pytest

from .speed_of_light import cli

pytestmark = pytest.mark.skipif(
    sys.platform != "linux" or os.geteuid() == 0,
    reason="inotify refuses to watch an unreadable directory, except for root",
)

CONFIG = """\
[[table]]
name    = "items"
glob    = "**/*.md"
ddl     = "CREATE TABLE items (name TEXT)"
on-file = "true"
"""


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def post_query(port, sql):
    """The status and JSON body `/query` answers with."""
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/query",
        data=json.dumps({"sql": sql}).encode(),
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


def describe_a_directory_the_server_cannot_watch():
    def it_fails_every_query_after_it_appears(tmp_path):
        (tmp_path / ".dirsql.toml").write_text(CONFIG)
        port = free_port()
        server = subprocess.Popen(
            [cli(), "server", "-c", ".dirsql.toml", "--port", str(port)],
            cwd=str(tmp_path),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            ready = server.stdout.readline()
            assert ready.startswith("Running at"), ready + server.stderr.read()
            assert post_query(port, "SELECT count(*) AS n FROM items") == (
                200,
                [{"n": 0}],
            )

            (tmp_path / "locked").mkdir(mode=0o000)
            time.sleep(1)

            answers = [
                post_query(port, "SELECT count(*) AS n FROM items") for _ in range(2)
            ]
        finally:
            server.terminate()
            server.wait(timeout=60)

        for status, body in answers:
            assert status == 500, body
            assert "Permission denied" in body["error"], body
