"""CLI e2e: `SIGINT` ends a run that is still in progress.

Spawns the real launcher over a real config whose `on-file` hook blocks, so
the core is provably mid-scan when the signal lands, then asserts the process
dies of it. No mocks: real process, real filesystem, real core.
"""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import time

import pytest

_LAUNCH = "import sys; from dirsql.cli.main import main; sys.exit(main())"


def _wait_until_scanning(proc, ready, timeout=30.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if ready.exists():
            return
        assert proc.poll() is None, "the run exited before the scan began"
        time.sleep(0.05)
    raise AssertionError("the scan never reached the on-file hook")


def describe_sigint_during_a_scan():
    @pytest.fixture
    def scanning(tmp_path):
        root = tmp_path / "data"
        root.mkdir()
        (root / "a.txt").write_text("hello")
        ready = tmp_path / "scanning"
        cfg = root / ".dirsql.toml"
        cfg.write_text(
            "[[table]]\n"
            'name = "files"\n'
            'ddl = "CREATE TABLE files (path TEXT)"\n'
            'glob = "*.txt"\n'
            f'on-file = "sh -c \'touch {ready}; sleep 120; printf \\"[]\\"\'"\n'
        )

        proc = subprocess.Popen(
            [
                sys.executable,
                "-c",
                _LAUNCH,
                "--config",
                str(cfg),
                "SELECT * FROM files",
            ],
            cwd=str(root),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=True,
        )
        try:
            yield proc, ready
        finally:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except OSError:
                pass
            proc.wait(timeout=10)

    def it_kills_the_process(scanning):
        proc, ready = scanning
        _wait_until_scanning(proc, ready)
        proc.send_signal(signal.SIGINT)
        assert proc.wait(timeout=15) == -signal.SIGINT
