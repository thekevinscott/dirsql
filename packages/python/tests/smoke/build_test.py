"""Packaging smoke test for the published PyPI wheel -- the `smoke` tier
(build -> pack -> install -> run).

Stages the real cargo-built `dirsql` binary under `dirsql/_binary/` the
way the release pipeline's `bundle_cli` step would, builds the wheel with
`maturin build`, installs it into a fresh venv with `pip install`, and
runs the installed `dirsql --version` console script plus an
`import dirsql` of the installed package. No mocks -- exactly what
`pip install dirsql` gives an end user.

Caveats:
- Tests only the host triple/interpreter. Cross-target coverage lives in
  the release pipeline's install matrix (one runner per target).
- The binary staging is reconstructed locally because it is normally
  performed by the release tool. The staged path is the one
  `dirsql.cli.binary_path()` consumes, so this still exercises the real
  launcher resolution path.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys

import pytest

_PY_PKG = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
_REPO_ROOT = os.path.abspath(os.path.join(_PY_PKG, "..", ".."))
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

# Where maturin's wheel `include` (pyproject `dirsql/_binary/*`) picks the
# binary up from, and where the installed launcher's `binary_path()` looks.
_BINARY_STAGE_DIR = os.path.join(_PY_PKG, "dirsql", "_binary")

_BIN_SUBDIR = "Scripts" if os.name == "nt" else "bin"


@pytest.fixture(scope="module")
def installed_venv(tmp_path_factory):
    """Build the wheel and install it into a fresh venv; yield its bin dir.

    Module-scoped: the maturin build + venv install is expensive, and both
    tests exercise the same installed artifact.
    """
    maturin = shutil.which("maturin")
    assert maturin is not None, (
        "`maturin` not on PATH -- run `uv sync` in packages/python"
    )
    assert os.path.exists(_BINARY), (
        f"prerequisite missing: dirsql binary not built at {_BINARY}; "
        "run `cargo build -p dirsql --features cli` first"
    )

    staging = tmp_path_factory.mktemp("dirsql-smoke")

    # 1. Stage the cargo-built CLI binary where the wheel `include` expects it.
    os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
    staged_binary = os.path.join(_BINARY_STAGE_DIR, "dirsql")
    shutil.copy(_BINARY, staged_binary)
    os.chmod(staged_binary, 0o755)
    try:
        # 2. Build the real wheel through the package's actual maturin config.
        wheel_dir = staging / "dist"
        build = subprocess.run(
            [maturin, "build", "--out", str(wheel_dir)],
            cwd=_PY_PKG,
            capture_output=True,
            text=True,
            timeout=1800,
        )
        assert build.returncode == 0, (
            f"maturin build failed:\n{build.stdout}\n{build.stderr}"
        )
        wheels = [f for f in os.listdir(wheel_dir) if f.endswith(".whl")]
        assert len(wheels) == 1, f"expected exactly one wheel, saw {wheels}"
        wheel = str(wheel_dir / wheels[0])

        # 3. Fresh venv, exactly what an end user installs into.
        venv_dir = staging / "venv"
        made = subprocess.run(
            [sys.executable, "-m", "venv", str(venv_dir)],
            capture_output=True,
            text=True,
            timeout=300,
        )
        assert made.returncode == 0, f"venv creation failed:\n{made.stderr}"
        venv_bin = str(venv_dir / _BIN_SUBDIR)

        install = subprocess.run(
            [os.path.join(venv_bin, "pip"), "install", "--no-input", wheel],
            capture_output=True,
            text=True,
            timeout=600,
        )
        assert install.returncode == 0, f"pip install failed:\n{install.stderr}"

        yield venv_bin
    finally:
        shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)


def describe_pip_installed_dirsql():
    def it_registers_a_dirsql_console_script_that_runs_the_bundled_binary(
        installed_venv,
    ):
        cli = os.path.join(installed_venv, "dirsql")
        assert os.path.exists(cli), f"console script missing at {cli}"
        proc = subprocess.run(
            [cli, "--version"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert proc.returncode == 0, (
            f"expected exit 0; stdout={proc.stdout!r}, stderr={proc.stderr!r}"
        )
        assert "dirsql" in proc.stdout

    def it_imports_the_installed_sdk_package(installed_venv, tmp_path):
        python = os.path.join(installed_venv, "python")
        # cwd is a scratch dir so `import dirsql` resolves the installed
        # wheel, never the source tree.
        proc = subprocess.run(
            [python, "-c", "import dirsql; print(dirsql.__version__)"],
            cwd=str(tmp_path),
            capture_output=True,
            text=True,
            timeout=60,
        )
        assert proc.returncode == 0, (
            f"expected exit 0; stdout={proc.stdout!r}, stderr={proc.stderr!r}"
        )
        assert proc.stdout.strip(), "expected a non-empty __version__"
