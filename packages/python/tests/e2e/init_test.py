"""E2E test for `dirsql init` through the Python launcher.

`init` writes a fixed starter `.dirsql.toml` -- the same single `files`
table zero-config mode serves -- regardless of the target directory's
contents. No LLM, no network, no filesystem inspection. No mocking of any
kind: spawns the real ``dirsql`` console script as a subprocess against the
real built binary.
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

# The exact, fixed starter config `init` writes -- byte-for-byte the same
# `[[table]]` block the zero-config default (`default_files_table` in
# `packages/rust/src/bin/dirsql.rs`) uses.
_EXPECTED_TOML = (
    '[[table]]\nddl  = "CREATE TABLE files '
    "(_path TEXT, _basename TEXT, _dir TEXT, _ext TEXT, "
    '_size INTEGER, _mtime INTEGER, _ctime INTEGER)"\nglob = "**/*"\n'
)


def _cli() -> str:
    """Resolve the `dirsql` console script for this test env."""
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return dirsql


def describe_dirsql_init():
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

    def it_writes_the_fixed_default_files_table_config(staged_binary, tmp_path):
        proc = subprocess.run(
            [_cli(), "init"],
            cwd=tmp_path,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=30,
        )
        assert proc.returncode == 0, (
            f"expected exit 0; stdout={proc.stdout!r}, stderr={proc.stderr!r}"
        )

        config = tmp_path / ".dirsql.toml"
        assert config.exists()
        assert config.read_text() == _EXPECTED_TOML

    def it_produces_the_same_output_regardless_of_directory_contents(
        staged_binary, tmp_path
    ):
        (tmp_path / "notes.txt").write_text("hello")
        (tmp_path / "data.json").write_text('{"a": 1}')
        (tmp_path / "nested").mkdir()
        (tmp_path / "nested" / "a.md").write_text("hi")

        proc = subprocess.run(
            [_cli(), "init"],
            cwd=tmp_path,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=30,
        )
        assert proc.returncode == 0

        config = tmp_path / ".dirsql.toml"
        assert config.read_text() == _EXPECTED_TOML

    def it_refuses_to_overwrite_an_existing_config(staged_binary, tmp_path):
        (tmp_path / ".dirsql.toml").write_text("# old\n")

        proc = subprocess.run(
            [_cli(), "init"],
            cwd=tmp_path,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=30,
        )
        assert proc.returncode != 0
        assert (tmp_path / ".dirsql.toml").read_text() == "# old\n"

    def it_overwrites_with_force(staged_binary, tmp_path):
        (tmp_path / ".dirsql.toml").write_text("# old\n")

        proc = subprocess.run(
            [_cli(), "init", "--force"],
            cwd=tmp_path,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=30,
        )
        assert proc.returncode == 0, (
            f"expected exit 0; stdout={proc.stdout!r}, stderr={proc.stderr!r}"
        )
        assert (tmp_path / ".dirsql.toml").read_text() == _EXPECTED_TOML
