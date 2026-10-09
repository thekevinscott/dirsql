"""Watch speed: the time from a file write to its event reaching the consumer.

Two identical small trees of JSON-lines files, one per side. Native is `inotifywait`
recursively watching `close_write`, a competent user's watcher, whose consumer
reads the changed file and parses it. dirsql is `dirsql server` holding an
`/events` stream open, whose consumer waits for the changed row. Each sample
writes a new value into one file and stops the clock when the consumer has the
value, so both sides deliver the same fact. Samples alternate between the two
sides. No mocks: real console script, real server process, real sockets, real
filesystem.
"""

from __future__ import annotations

import json
import shutil
import socket
import subprocess
import time
from itertools import count

import pytest

from .speed_of_light import (
    Startup,
    assert_speed_of_light,
    cli,
    paired,
)

DIRS = 2
FILES_PER_DIR = 10
PAIRS = 15
DEADLINE = 30.0
WATCHES_ESTABLISHED = 1.0
NO_STARTUP = Startup(0.0, 0.0)

CONFIG = """\
[[table]]
name    = "rows"
glob    = "**/*.jl"
ddl     = "CREATE TABLE rows (v INTEGER)"
on-file = "cat"
"""


def build_tree(root):
    for d in range(DIRS):
        folder = root / f"d{d}"
        folder.mkdir(parents=True)
        for f in range(FILES_PER_DIR):
            (folder / f"f{f}.jl").write_text('{"v": 0}\n')


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def target(root, sample):
    return root / f"d{sample * 7 % DIRS}" / f"f{sample % FILES_PER_DIR}.jl"


class Writer:
    """Writes a fresh value into a rotating file and times its delivery."""

    def __init__(self, root):
        self.root = root
        self.samples = count(1)

    def write(self):
        sample = next(self.samples)
        value = sample
        started = time.perf_counter()
        target(self.root, sample).write_text(f'{{"v": {value}}}\n')
        return value, started


class Native:
    def __init__(self, root):
        self.writer = Writer(root)
        script = (
            "inotifywait -m -r -q -e close_write --format '%w%f' \"$1\""
            ' | while IFS= read -r path; do cat "$path"; done'
        )
        self.proc = subprocess.Popen(
            ["bash", "-c", script, "bash", str(root)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        time.sleep(WATCHES_ESTABLISHED)

    def sample(self):
        value, started = self.writer.write()
        while json.loads(self.proc.stdout.readline())["v"] != value:
            pass
        return value, time.perf_counter() - started

    def close(self):
        self.proc.terminate()
        self.proc.wait(timeout=60)


class Dirsql:
    def __init__(self, root):
        self.writer = Writer(root)
        (root / ".dirsql.toml").write_text(CONFIG)
        port = free_port()
        self.proc = subprocess.Popen(
            [cli(), "server", "-c", ".dirsql.toml", "--port", str(port)],
            cwd=str(root),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        ready = self.proc.stdout.readline()
        assert ready.startswith("Running at"), ready
        self.sock = socket.create_connection(("127.0.0.1", port))
        self.sock.settimeout(DEADLINE)
        self.sock.sendall(b"GET /events HTTP/1.1\r\nHost: x\r\n\r\n")
        self.buffer = b""
        self.next_event()
        while b"event: ready" not in self.buffer:
            self.next_event()
        self.buffer = b""

    def next_event(self):
        self.buffer += self.sock.recv(65536)

    def sample(self):
        value, started = self.writer.write()
        needle = f'"row":{{"v":{value}}}'.encode()
        while needle not in self.buffer:
            self.next_event()
        seconds = time.perf_counter() - started
        self.buffer = b""
        return value, seconds

    def close(self):
        self.sock.close()
        self.proc.terminate()
        self.proc.wait(timeout=60)


def describe_watch_speed_of_light():
    @pytest.fixture(scope="module")
    def trees(tmp_path_factory):
        base = tmp_path_factory.mktemp("watch-speed")
        native_root, dirsql_root = base / "native", base / "dirsql"
        for root in (native_root, dirsql_root):
            build_tree(root)
        native = Native(native_root)
        dirsql = Dirsql(dirsql_root)
        try:
            yield native, dirsql
        finally:
            dirsql.close()
            native.close()
            shutil.rmtree(base, ignore_errors=True)

    def it_delivers_a_write_to_the_consumer_within_the_bar(trees):
        native, dirsql = trees
        result = paired(
            native.sample, lambda _timeout: dirsql.sample(), NO_STARTUP, PAIRS
        )
        assert result.native_rows == next(native.writer.samples) - 1
        assert result.dirsql_rows == next(dirsql.writer.samples) - 1
        assert_speed_of_light("watch", result, NO_STARTUP)
