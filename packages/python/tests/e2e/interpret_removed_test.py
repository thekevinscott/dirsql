"""E2E test asserting `dirsql interpret` is gone (epic #321 / #323).

The native-language (`.py`) config path and its `interpret` handshake
were hard-removed from the Python SDK. The launcher no longer intercepts
``interpret``; it forwards all argv to the bundled Rust binary, which
rejects the unknown subcommand (clap, non-zero exit).

No mocking of any kind: this spawns the real ``dirsql`` console script as
a subprocess against the real built binary and asserts the failure is a
clean clap error, not a Python traceback.
"""

from __future__ import annotations

import shutil
import subprocess


def _cli() -> str:
    """Resolve the `dirsql` console script for this test env.

    Failing loudly here surfaces an environment misconfiguration rather
    than masking it as a test assertion failure further down.
    """
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return dirsql


def describe_dirsql_interpret_removed():
    def it_exits_nonzero_for_the_interpret_subcommand():
        proc = subprocess.run(
            [_cli(), "interpret", "some-config.py"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
        assert proc.returncode != 0, (
            f"expected non-zero exit; stdout={proc.stdout!r}, "
            f"stderr={proc.stderr!r}"
        )

    def it_fails_with_a_clean_error_not_a_python_traceback():
        proc = subprocess.run(
            [_cli(), "interpret", "some-config.py"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
        # A clap "unknown subcommand" error, not a leaked Python traceback.
        assert "Traceback" not in proc.stderr
        assert proc.stderr.strip() != ""
