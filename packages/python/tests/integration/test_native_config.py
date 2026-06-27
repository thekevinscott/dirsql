"""CLI integration test for native Python config file support.

Spawns the bundled Rust binary directly against the
``__fixtures__/dirsql.config.py`` fixture and asserts the HTTP server
serves the ``papers`` table. Bypasses the Python launcher because the
architectural property under test ("the Rust binary dispatches
non-TOML configs to ``dirsql interpret``") is the binary's job — the
launcher is a transparent forwarder.

The binary's `dirsql interpret <X>` subprocess resolves via PATH; in
this dev/CI tree the pip-installed `dirsql` console script (entry
point ``dirsql.cli.main:main``) is on PATH and handles the
``interpret`` subcommand directly.
"""

import json
import os
import socket
import subprocess
import time
import urllib.error
import urllib.request

FIXTURE_DIR = os.path.join(os.path.dirname(__file__), "__fixtures__")
CONFIG_PATH = os.path.join(FIXTURE_DIR, "dirsql.config.py")
# A native config that omits `root` entirely (issue #251). Its parent dir
# holds matching `data/**/meta.json`, so the scan root must default to that
# parent -- the same behavior a `.dirsql.toml` already has.
NOROOT_CONFIG_PATH = os.path.join(FIXTURE_DIR, "noroot", "dirsql.config.py")

# Workspace root → cargo's target dir. The CI workflow builds the
# binary with `cargo build --release -p dirsql --features cli` before
# running integration tests; locally, `cargo build --release` does the
# same. Fall back to debug if release isn't present.
_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]


def _wait_for_server(proc: subprocess.Popen, port: int, timeout: float = 5.0) -> bool:
    """Return True once the server accepts a connection, False if the process
    exits first or the timeout elapses."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            return False
        try:
            urllib.request.urlopen(f"http://localhost:{port}/query", timeout=0.5)
            return True
        except urllib.error.HTTPError:
            return True  # any HTTP response means the server is up
        except Exception:
            time.sleep(0.05)
    return False


def _query(port: int, sql: str) -> list:
    body = json.dumps({"sql": sql}).encode()
    req = urllib.request.Request(
        f"http://localhost:{port}/query",
        data=body,
        headers={"content-type": "application/json"},
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())


def _serve_and_query(config_path: str, sql: str) -> list:
    """Spawn the binary against ``config_path``, run ``sql``, return rows."""
    assert os.path.exists(BINARY), (
        f"dirsql binary not built at {BINARY}; "
        "run `cargo build --release -p dirsql --features cli` first"
    )
    port = _free_port()
    proc = subprocess.Popen(
        [
            BINARY,
            "--config",
            config_path,
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
        ],
        stderr=subprocess.PIPE,
    )
    try:
        assert _wait_for_server(proc, port), (
            f"dirsql server did not start with --config {config_path}"
        )
        return _query(port, sql)
    finally:
        proc.terminate()
        proc.wait(timeout=5)


def describe_cli_py_config():
    def it_starts_an_http_server_serving_the_config_tables():
        rows = _serve_and_query(
            CONFIG_PATH, "SELECT title FROM papers ORDER BY title"
        )
        assert rows == [{"title": "Alpha"}, {"title": "Beta"}]

    def it_defaults_root_to_config_dir_when_root_is_omitted():
        # Issue #251: a native config with no explicit `root` must scan the
        # config file's parent directory (where `data/**/meta.json` lives),
        # matching how a `.dirsql.toml` defaults its root. Before the fix the
        # interpret child raised "requires either a root directory or a
        # config" and the server returned HTTP 503.
        rows = _serve_and_query(
            NOROOT_CONFIG_PATH, "SELECT title FROM papers ORDER BY title"
        )
        assert rows == [{"title": "Alpha"}, {"title": "Beta"}]
