"""E2E test for `dirsql --version` through the Python launcher (#294 parity).

Mirrors the Rust CLI e2e `version_flag_prints_and_exits_zero`
(packages/rust/tests/cli_e2e.rs) and the functional half of the TS smoke
test's `dirsql --version` run: the Python console script must find the
bundled Rust binary, forward argv to it, and surface its output and exit
code unchanged.

No mocking of any kind: this spawns the real ``dirsql`` console script as
a subprocess against the real built binary (staged where the launcher's
``binary_path()`` looks, exactly as the extension-package e2e does).
"""

from __future__ import annotations

import os
import shutil
import subprocess

import pytest

import dirsql as _dirsql_pkg

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

# Where the launcher's `binary_path()` looks: `<dirsql package>/_binary/dirsql`.
_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")


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


def describe_dirsql_version_flag():
    @pytest.fixture
    def staged_binary():
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged)
        os.chmod(staged, 0o755)
        try:
            yield staged
        finally:
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)

    def it_prints_the_version_and_exits_zero(staged_binary):
        proc = subprocess.run(
            [_cli(), "--version"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=30,
        )
        assert proc.returncode == 0, (
            f"expected exit 0; stdout={proc.stdout!r}, stderr={proc.stderr!r}"
        )
        assert "dirsql" in proc.stdout
