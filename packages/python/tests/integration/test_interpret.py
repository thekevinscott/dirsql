"""Integration tests for `dirsql interpret` -- the long-running native
config helper (#196).

Spawns the real `dirsql` console script as a subprocess and talks NDJSON
over stdin/stdout. No monkeypatching, no in-process shortcut.

NDJSON protocol (per #196):

  handshake (helper -> caller, once on startup):
    {"type": "config", "state": <vars(app)>}

  extract request (caller -> helper):
    {"type": "extract", "id": <int>, "table": "<name>", "path": "<abs>"}

  extract response (helper -> caller):
    {"type": "result", "id": <int>, "ok": true,  "rows": [...]}
    {"type": "result", "id": <int>, "ok": false, "error": "<msg>"}
"""

from __future__ import annotations

import json
import shutil
import subprocess
import threading
from pathlib import Path

FIXTURE_DIR = Path(__file__).parent / "__fixtures__" / "interpret"
HAPPY_CONFIG = FIXTURE_DIR / "dirsql.config.py"
RAISES_CONFIG = FIXTURE_DIR / "dirsql.config_raises.py"
NO_APP_CONFIG = FIXTURE_DIR / "dirsql.config_no_app.py"
ALPHA_PATH = FIXTURE_DIR / "data" / "a" / "meta.json"


def _cli_argv() -> list[str]:
    """Resolve the `dirsql` console-script invocation for this test env.

    The console script is the production entry point; `uv run maturin
    develop` installs it into the active venv before tests run. Failing
    loudly here surfaces an environment misconfiguration rather than
    masking it as a test assertion failure.
    """
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return [dirsql]


def _spawn(config_path: Path) -> subprocess.Popen:
    return subprocess.Popen(
        [*_cli_argv(), "interpret", str(config_path)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )


def _readline(proc: subprocess.Popen, timeout: float = 5.0) -> str:
    """Read one stdout line with a timeout.

    On timeout, kill the process and surface stderr so the failure
    message is actionable rather than an opaque hang.
    """
    result: list[str | None] = [None]

    def reader() -> None:
        assert proc.stdout is not None
        result[0] = proc.stdout.readline()

    t = threading.Thread(target=reader, daemon=True)
    t.start()
    t.join(timeout)
    if t.is_alive():
        proc.kill()
        stderr = proc.stderr.read() if proc.stderr else ""
        raise AssertionError(
            f"timed out waiting for stdout line; stderr was:\n{stderr}"
        )
    line = result[0] or ""
    if not line:
        # EOF: process exited before producing a line.
        stderr = proc.stderr.read() if proc.stderr else ""
        raise AssertionError(
            f"helper exited (code={proc.returncode}) before writing a line; "
            f"stderr was:\n{stderr}"
        )
    return line


def _send(proc: subprocess.Popen, msg: dict) -> None:
    assert proc.stdin is not None
    proc.stdin.write(json.dumps(msg) + "\n")
    proc.stdin.flush()


def _shutdown(proc: subprocess.Popen) -> None:
    """Close stdin and wait for the helper to exit cleanly."""
    if proc.stdin is not None:
        try:
            proc.stdin.close()
        except BrokenPipeError:
            pass
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=2)


def test_handshake_matches_vars_app():
    """`state` in the handshake equals `vars(app)` -- tied to the dict
    serialization landed in #194/#197, not hardcoded field-by-field."""
    proc = _spawn(HAPPY_CONFIG)
    try:
        msg = json.loads(_readline(proc))
        assert msg["type"] == "config"
        state = msg["state"]
        # Same keys as `vars(DirSQL(...))` per test_serialization.py.
        assert set(state.keys()) == {
            "root",
            "tables",
            "ignore",
            "persist",
            "persist_path",
        }
        assert state["root"] == str(FIXTURE_DIR / "data")
        assert len(state["tables"]) == 1
        assert state["tables"][0]["ddl"] == "CREATE TABLE papers (title TEXT)"
        assert state["tables"][0]["glob"] == "**/meta.json"
        assert state["tables"][0]["strict"] is False
        assert state["ignore"] == []
        assert state["persist"] is False
        assert state["persist_path"] is None
    finally:
        _shutdown(proc)


def test_single_extract():
    """One extract request -> one `ok: true` response with the fixture rows."""
    proc = _spawn(HAPPY_CONFIG)
    try:
        _readline(proc)  # handshake
        _send(
            proc,
            {
                "type": "extract",
                "id": 1,
                "table": "papers",
                "path": str(ALPHA_PATH),
            },
        )
        response = json.loads(_readline(proc))
        assert response["type"] == "result"
        assert response["id"] == 1
        assert response["ok"] is True
        assert response["rows"] == [{"title": "Alpha"}]
    finally:
        _shutdown(proc)


def test_extract_error():
    """An exception in user `extract` -> `ok: false` with the error message."""
    proc = _spawn(RAISES_CONFIG)
    try:
        _readline(proc)  # handshake
        _send(
            proc,
            {
                "type": "extract",
                "id": 7,
                "table": "papers",
                "path": str(ALPHA_PATH),
            },
        )
        response = json.loads(_readline(proc))
        assert response["type"] == "result"
        assert response["id"] == 7
        assert response["ok"] is False
        assert "synthetic extract failure" in response["error"]
    finally:
        _shutdown(proc)


def test_unknown_table():
    """Request for a table the config never declared -> `ok: false`."""
    proc = _spawn(HAPPY_CONFIG)
    try:
        _readline(proc)  # handshake
        _send(
            proc,
            {
                "type": "extract",
                "id": 3,
                "table": "nonexistent",
                "path": str(ALPHA_PATH),
            },
        )
        response = json.loads(_readline(proc))
        assert response["type"] == "result"
        assert response["id"] == 3
        assert response["ok"] is False
        assert "nonexistent" in response["error"]
    finally:
        _shutdown(proc)


def test_startup_failure_no_app():
    """Config without a module-level `app` exits non-zero with clean stderr."""
    proc = subprocess.Popen(
        [*_cli_argv(), "interpret", str(NO_APP_CONFIG)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    stdout, stderr = proc.communicate(timeout=10)
    assert proc.returncode != 0, (
        f"expected non-zero exit; stdout={stdout!r}, stderr={stderr!r}"
    )
    # "Clean": a single human-readable line, not a Python traceback.
    assert "Traceback" not in stderr
    assert "app" in stderr.lower()
