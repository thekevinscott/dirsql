"""Binding tier: resolve a SQLite extension by bare package name (#298).

Lays a real compiled loadable inside a real package directory on `sys.path`,
points `DirSQL` at it by **bare package name**, and asserts the SDK resolves
the actual on-disk file, loads it, and the function it registers is callable.
Real layout, no mocks -- the shape mirrors the TypeScript sibling (#299) and
the Rust end-to-end test: resolve -> load -> callable.

The loadable is the repo's `tests/fixtures/testext` cdylib (registers
`dirsql_testext_answer() -> 42`), built on the fly with cargo.
"""

import importlib
import json
import os
import shutil
import subprocess
import sys

import pytest

from dirsql import DirSQL

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_FIXTURE_MANIFEST = os.path.join(
    _REPO_ROOT, "packages", "rust", "tests", "fixtures", "testext", "Cargo.toml"
)


def _build_fixture_extension(target_dir):
    """Build the testext cdylib and return the path to its loadable file."""
    proc = subprocess.run(
        [
            os.environ.get("CARGO", "cargo"),
            "build",
            "--manifest-path",
            _FIXTURE_MANIFEST,
            "--target-dir",
            target_dir,
            "--message-format=json",
        ],
        capture_output=True,
        text=True,
        env={
            k: v
            for k, v in os.environ.items()
            if k not in ("RUSTFLAGS", "RUSTDOCFLAGS")
        },
    )
    assert proc.returncode == 0, f"fixture build failed:\n{proc.stderr}"
    artifact = None
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") == "compiler-artifact":
            for f in msg.get("filenames") or []:
                if f.endswith((".so", ".dylib", ".dll")):
                    artifact = f
    assert artifact, "no cdylib artifact in cargo build output"
    return artifact


def describe_extension_by_package_name():
    @pytest.mark.asyncio
    async def it_resolves_loads_and_calls_an_extension_by_package_name(tmp_path):
        if shutil.which(os.environ.get("CARGO", "cargo")) is None:
            pytest.skip("cargo not available to build the extension fixture")

        so = _build_fixture_extension(str(tmp_path / "target"))

        # A real installed-package layout:
        # <env>/dirsql_testext_pkg/{__init__.py, *.so}
        env = tmp_path / "env"
        pkg = env / "dirsql_testext_pkg"
        pkg.mkdir(parents=True)
        (pkg / "__init__.py").write_text("")
        shutil.copy(so, pkg / os.path.basename(so))

        sys.path.insert(0, str(env))
        importlib.invalidate_caches()
        try:
            db = DirSQL(
                str(tmp_path),
                extensions=[
                    {
                        "path": "dirsql_testext_pkg",
                        "entrypoint": "sqlite3_extension_init",
                    }
                ],
            )
            await db.ready()
            assert await db.query("SELECT dirsql_testext_answer() AS a") == [{"a": 42}]
        finally:
            sys.path.remove(str(env))
            sys.modules.pop("dirsql_testext_pkg", None)
            importlib.invalidate_caches()
