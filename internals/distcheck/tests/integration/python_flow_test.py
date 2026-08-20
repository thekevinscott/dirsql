"""Integration tier: the python packaging distcheck flow against real subprocesses.

Runs `gate.run` with its default `subprocess.run` / real `FileSystem` -- the
build -> pack -> install -> run behavior the old per-package packaging
suite exercised. Skips when the build prerequisite (`maturin`) is absent,
since the flow's actual CI execution is the `dirsql-distcheck python` job,
which installs it first.
"""
from __future__ import annotations

import os
import shutil

import pytest

from distcheck.python_flow.gate import run

_REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))
_PKG = os.path.join(_REPO, "packages", "python")


def test_pip_installed_wheel_builds_installs_and_runs():
    if shutil.which("maturin") is None:
        pytest.skip("maturin not on PATH (uv sync in packages/python)")

    assert run(_PKG, _REPO) == 0
