"""E2E test for `dirsql --version` through the Python launcher.

Spawns the real console script (which runs the CLI in-process through the
compiled extension, #738) and asserts it prints the version of the package it
was installed from. The embedded core crate's version is not that -- only the
crates.io lane rewrites that literal, so wheels published at 0.4.x printed
`dirsql 0.2.7` (#958).
"""

from __future__ import annotations

import shutil
import subprocess

import dirsql


def _cli() -> str:
    """Resolve the `dirsql` console script for this test env."""
    dirsql_bin = shutil.which("dirsql")
    assert dirsql_bin is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return dirsql_bin


def describe_dirsql_version_flag():
    def it_prints_the_installed_packages_version_and_exits_zero():
        proc = subprocess.run(
            [_cli(), "--version"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert proc.returncode == 0, (
            f"expected exit 0; stdout={proc.stdout!r}, stderr={proc.stderr!r}"
        )
        assert proc.stdout.strip() == f"dirsql {dirsql.__version__}"
