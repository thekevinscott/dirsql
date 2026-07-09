"""Integration tier: the python packaging distcheck flow against real subprocesses.

Runs `gate.run` with its default `subprocess.run` / real `FileSystem` -- the
build -> pack -> install -> run behavior the old per-package packaging
suite exercised. Skips when the build prerequisites (the cargo `dirsql` binary,
`maturin`) are absent, since the flow's actual CI execution is the
`dirsql-distcheck python` job, which builds them first.
"""
from __future__ import annotations

import os
import shutil

import pytest

from distcheck.python_flow.gate import run

_REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))
_PKG = os.path.join(_REPO, "packages", "python")


def test_pip_installed_wheel_builds_installs_and_runs():
    binary = next(
        (
            os.path.join(_REPO, "target", profile, "dirsql")
            for profile in ("release", "debug")
            if os.path.exists(os.path.join(_REPO, "target", profile, "dirsql"))
        ),
        None,
    )
    if binary is None:
        pytest.skip("dirsql CLI binary not built (cargo build -p dirsql --features cli)")
    if shutil.which("maturin") is None:
        pytest.skip("maturin not on PATH (uv sync in packages/python)")

    assert run(_PKG, _REPO) == 0
