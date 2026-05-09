"""End-to-end smoke test for the published Python wheel.

Builds the wheel against the host triple (with the `dirsql` CLI binary
staged inside it), installs the wheel into a fresh, isolated venv via
`uv`, and runs `dirsql --version` against the resulting console-script
entry point. No mocks, no monkeypatching, no fakes -- the asserts are
against a real PyPI-shaped artifact.

This test is the build-CI publishability gate: if it goes red, the wheel
on PyPI is broken (`uvx dirsql` would fail) and the release must not go
out.
"""

from __future__ import annotations

import subprocess
import sys
import venv
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[4]
PY_PKG = REPO / "packages" / "python"
STAGE_SCRIPT = REPO / "scripts" / "stage_cli_binary.py"


@pytest.fixture(scope="module")
def built_wheel(tmp_path_factory):
    """Stage the binary, build a wheel, return the wheel path. Module-scoped
    because the cargo build is expensive."""
    out = tmp_path_factory.mktemp("wheel-out")

    stage = subprocess.run([sys.executable, str(STAGE_SCRIPT)])
    assert stage.returncode == 0, "scripts/stage_cli_binary.py failed"

    build = subprocess.run(
        [
            "uv",
            "run",
            "maturin",
            "build",
            "--release",
            "--out",
            str(out),
        ],
        cwd=PY_PKG,
    )
    assert build.returncode == 0, "maturin build failed"

    wheels = list(out.glob("dirsql-*.whl"))
    assert len(wheels) == 1, f"expected exactly one wheel, found {wheels}"
    return wheels[0]


def describe_wheel_install():
    def it_registers_a_dirsql_console_script(built_wheel, tmp_path):
        venv_dir = tmp_path / "venv"
        venv.create(venv_dir, with_pip=True)
        py = venv_dir / "bin" / "python"
        pip = venv_dir / "bin" / "pip"

        install = subprocess.run(
            [str(pip), "install", str(built_wheel)],
            capture_output=True,
        )
        assert install.returncode == 0, install.stderr.decode()

        entry_check = subprocess.run(
            [
                str(py),
                "-c",
                "from importlib.metadata import entry_points; "
                "print('dirsql' in {ep.name for ep in entry_points(group='console_scripts')})",
            ],
            capture_output=True,
            text=True,
        )
        assert entry_check.returncode == 0, entry_check.stderr
        assert entry_check.stdout.strip() == "True", (
            "wheel does not register a `dirsql` console script -- "
            "[project.scripts] is missing from pyproject.toml or maturin "
            "stripped it"
        )

    def it_runs_dirsql_version_against_the_bundled_binary(built_wheel, tmp_path):
        venv_dir = tmp_path / "venv"
        venv.create(venv_dir, with_pip=True)
        pip = venv_dir / "bin" / "pip"
        dirsql = venv_dir / "bin" / "dirsql"

        install = subprocess.run(
            [str(pip), "install", str(built_wheel)],
            capture_output=True,
        )
        assert install.returncode == 0, install.stderr.decode()
        assert dirsql.is_file(), f"console script not at {dirsql}"

        result = subprocess.run(
            [str(dirsql), "--version"],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0, (
            f"dirsql --version failed: stdout={result.stdout!r} "
            f"stderr={result.stderr!r}"
        )
        # `clap` prints `dirsql <version>` on --version.
        assert "dirsql" in result.stdout, result.stdout
