"""Subprocess plumbing for the `dirsql interpret` integration tests.

Kept in its own module (rather than `conftest.py` or inline in the
test file) because these helpers are not pytest fixtures -- they are
plain functions that drive a child process via NDJSON. Importing them
keeps each test focused on the assertion, not the framing.

Naming intentionally does not match `test_*.py` / `*_test.py` so
pytest skips collection.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import threading
from pathlib import Path
from typing import Any


def cli_argv() -> list[str]:
    """Resolve the `dirsql` console-script invocation for this test env.

    Failing loudly here surfaces an environment misconfiguration rather
    than masking it as a test assertion failure further down.
    """
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return [dirsql]


def spawn(config_path: Path) -> subprocess.Popen:
    """Start `dirsql interpret <config>` with piped stdin/stdout/stderr."""
    return subprocess.Popen(
        [*cli_argv(), "interpret", str(config_path)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )


def readline(proc: subprocess.Popen, timeout: float = 5.0) -> str:
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
        stderr = proc.stderr.read() if proc.stderr else ""
        raise AssertionError(
            f"helper exited (code={proc.returncode}) before writing a line; "
            f"stderr was:\n{stderr}"
        )
    return line


def send(proc: subprocess.Popen, msg: dict[str, Any]) -> None:
    """Write one NDJSON line to the helper's stdin."""
    assert proc.stdin is not None
    proc.stdin.write(json.dumps(msg) + "\n")
    proc.stdin.flush()


def shutdown(proc: subprocess.Popen) -> None:
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
