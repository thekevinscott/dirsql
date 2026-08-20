"""CLI e2e: fan-out file->table matching through the real launcher + binary.

A file matching two tables' overlapping globs populates both tables; querying
each over the real `POST /query` surface returns the file's row (#580). No
mocks: real launcher, real binary, real process, real filesystem.
"""

import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

import pytest

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

import dirsql as _dirsql_pkg

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")

# An `on-file` hook emitting `path`, derived from the file path (`{path}`)
# relative to the scan root (`{root}`).
_HOOK_PATH = r"""on-file = '''sh -c 'rel=${1#"$2"/}; printf "[{\"path\":\"%s\"}]" "$rel"' sh {path} {root}'''"""


def _free_port():
    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]


def _wait_for_server(proc, port, timeout=5.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            return False
        try:
            urllib.request.urlopen(f"http://localhost:{port}/query", timeout=0.5)
            return True
        except urllib.error.HTTPError:
            return True
        except Exception:
            time.sleep(0.05)
    return False


def _post_sql(port, sql):
    req = urllib.request.Request(
        f"http://localhost:{port}/query",
        data=json.dumps({"sql": sql}).encode(),
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as err:
        return err.code, json.loads(err.read())


def describe_fanout():
    @pytest.fixture
    def server(tmp_path):
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged_binary = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged_binary)
        os.chmod(staged_binary, 0o755)

        root = tmp_path / "data"
        sub = root / "data" / "2401.00001"
        sub.mkdir(parents=True)
        (sub / "metadata.json").write_text("{}")
        cfg = root / ".dirsql.toml"
        cfg.write_text(
            "[[table]]\n"
            'name = "ta"\n'
            'ddl = "CREATE TABLE ta (path TEXT)"\n'
            'glob = "data/*/metadata.json"\n'
            f"{_HOOK_PATH}\n"
            "\n"
            "[[table]]\n"
            'name = "tb"\n'
            'ddl = "CREATE TABLE tb (path TEXT)"\n'
            'glob = "data/**/metadata.json"\n'
            f"{_HOOK_PATH}\n"
        )

        port = _free_port()
        proc = subprocess.Popen(
            [
                sys.executable,
                "-c",
                "import sys; from dirsql.cli.main import main; sys.exit(main())",
                "server",
                "--config",
                str(cfg),
                "--host",
                "127.0.0.1",
                "--port",
                str(port),
            ],
            cwd=str(root),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            ok = _wait_for_server(proc, port)
            if not ok:
                out, err = proc.communicate(timeout=5)
                raise AssertionError(
                    f"server did not start\n--- stdout ---\n{out}\n--- stderr ---\n{err}"
                )
            yield port
        finally:
            proc.terminate()
            proc.wait(timeout=5)
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)

    def it_populates_the_first_declared_table(server):
        status, rows = _post_sql(server, "SELECT path FROM ta")
        assert status == 200
        assert [r["path"] for r in rows] == ["data/2401.00001/metadata.json"]

    def it_also_populates_the_second_declared_table(server):
        status, rows = _post_sql(server, "SELECT path FROM tb")
        assert status == 200
        assert [r["path"] for r in rows] == ["data/2401.00001/metadata.json"]
