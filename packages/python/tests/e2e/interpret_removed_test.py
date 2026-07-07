"""E2E test asserting the `interpret` subcommand is rejected.

The launcher does not intercept ``interpret``; it forwards all argv to the
bundled Rust binary, which rejects the unknown subcommand (clap, non-zero
exit). No mocking of any kind: spawns the real ``dirsql`` console script as
a subprocess against the real built binary and asserts the failure is a
clean clap error, not a Python traceback.
"""

from __future__ import annotations

import shutil
import subprocess


def _cli() -> str:
    """Resolve the `dirsql` console script for this test env."""
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
            f"expected non-zero exit; stdout={proc.stdout!r}, stderr={proc.stderr!r}"
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
