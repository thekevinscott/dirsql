"""Packaging distcheck flow for the published PyPI wheel (#520; was
the per-package `build_test.py`).

build -> pack -> install -> run: build
the wheel with `maturin build`, install it into a fresh venv with `pip install`,
and run the installed `dirsql --version` console script plus an `import dirsql`.
No mocks -- exactly what `pip install dirsql` gives an end user.

Caveats:
- Tests only the host triple/interpreter. Cross-target coverage lives in the
  release pipeline's install matrix (one runner per target).
- No binary staging since #738: the wheel's extension module carries the CLI
  and the console script calls it in-process, so there is nothing to stage
  so this still exercises the real launcher resolution path.

Effects funnel through an injected `runner` (subprocess.run) and `fs`
(FileSystem) so every stage's command and failure handling is unit-testable
without spawning a real build.
"""
from __future__ import annotations

import os
import subprocess
import sys

from distcheck.filesystem import FileSystem


class DistcheckError(RuntimeError):
    """A distcheck stage failed -- carries a human-readable diagnostic."""


def _require_zero(result, message: str) -> None:
    """Raise `DistcheckError(message)` unless `result` exited 0."""
    if result.returncode != 0:
        raise DistcheckError(message)


def bin_subdir(os_name: str = os.name) -> str:
    """venv scripts directory name -- `Scripts` on Windows, `bin` elsewhere."""
    return {"nt": "Scripts"}.get(os_name, "bin")


def sole_wheel(names) -> str:
    """The single `.whl` among `names`, or raise -- the build must emit one."""
    wheels = [name for name in names if name.endswith(".whl")]
    if len(wheels) != 1:
        raise DistcheckError(f"expected exactly one wheel, saw {wheels}")
    (wheel,) = wheels
    return wheel


def check_wheel_tag(wheel: str) -> None:
    """Assert the stable-ABI (abi3) tag (#487): one `cp3x-abi3` wheel per
    platform, not a version-locked `cpXY-cpXY` that re-inflates the release
    matrix 4x."""
    if "-abi3-" not in wheel:
        raise DistcheckError(f"expected an abi3 wheel tag, saw {wheel!r}")
    interp = wheel.split("-")[2]  # dirsql-<ver>-<interp>-<abi>-<plat>.whl
    if not interp.startswith("cp3"):
        raise DistcheckError(f"unexpected interpreter tag in {wheel!r}")


def run(
    pkg_root: str,
    repo_root: str,
    *,
    maturin: str = "maturin",
    runner=subprocess.run,
    fs: FileSystem = FileSystem(),
) -> int:
    staging = fs.mkdtemp("dirsql-distcheck-")
    try:
        wheel_dir = os.path.join(staging, "dist")
        fs.makedirs(wheel_dir)
        build = runner(
            [maturin, "build", "--out", wheel_dir],
            cwd=pkg_root,
            capture_output=True,
            text=True,
        )
        _require_zero(build, f"maturin build failed:\n{build.stdout}\n{build.stderr}")

        wheel_name = sole_wheel(fs.listdir(wheel_dir))
        check_wheel_tag(wheel_name)
        wheel = os.path.join(wheel_dir, wheel_name)

        venv_dir = os.path.join(staging, "venv")
        made = runner(
            [sys.executable, "-m", "venv", venv_dir],
            capture_output=True,
            text=True,
        )
        _require_zero(made, f"venv creation failed:\n{made.stderr}")
        venv_bin = os.path.join(venv_dir, bin_subdir())

        install = runner(
            [os.path.join(venv_bin, "pip"), "install", "--no-input", wheel],
            capture_output=True,
            text=True,
        )
        _require_zero(install, f"pip install failed:\n{install.stderr}")

        cli = os.path.join(venv_bin, "dirsql")
        if not fs.exists(cli):
            raise DistcheckError(f"console script missing at {cli}")
        version = runner(
            [cli, "--version"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
        )
        version_err = (
            f"`dirsql --version` failed; "
            f"stdout={version.stdout!r}, stderr={version.stderr!r}"
        )
        _require_zero(version, version_err)
        if "dirsql" not in version.stdout:
            raise DistcheckError(version_err)

        # cwd is the scratch staging dir so `import dirsql` resolves the
        # installed wheel, never the source tree.
        python = os.path.join(venv_bin, "python")
        imported = runner(
            [python, "-c", "import dirsql; print(dirsql.__version__)"],
            cwd=staging,
            capture_output=True,
            text=True,
        )
        imported_err = (
            f"`import dirsql` failed; "
            f"stdout={imported.stdout!r}, stderr={imported.stderr!r}"
        )
        _require_zero(imported, imported_err)
        if not imported.stdout.strip():
            raise DistcheckError(imported_err)
    finally:
        fs.rmtree(staging)
    return 0
