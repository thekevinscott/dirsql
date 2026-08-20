"""Integration tier: the node packaging distcheck flow against real subprocesses.

Runs `gate.run` with its default `subprocess.run` / real `FileSystem` -- the
build -> pack -> install -> run behavior the old per-package packaging
suite exercised. Skips when the build prerequisites (the staged host addon
from `pnpm build`, `npm`/`pnpm`) are absent, since the flow's actual CI
execution is the `dirsql-distcheck node` job, which builds them first.
"""
from __future__ import annotations

import os
import platform
import shutil
import sys

import pytest

from distcheck.node_flow.gate import run, staged_addon_path
from distcheck.node_flow.platforms import detect_host

_REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))
_TS = os.path.join(_REPO, "packages", "ts")


def test_npm_installed_cli_packs_installs_and_runs():
    host = detect_host(sys.platform, platform.machine())
    if not os.path.exists(staged_addon_path(_TS, host)):
        pytest.skip("staged addon absent (pnpm build in packages/ts)")
    if shutil.which("npm") is None or shutil.which("pnpm") is None:
        pytest.skip("npm/pnpm not on PATH")

    assert run(_TS, host) == 0
