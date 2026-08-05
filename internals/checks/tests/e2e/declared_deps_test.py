"""E2E test for `dirsql-checks declared-deps` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script as a
subprocess against a real scratch package tree on a real filesystem.
"""

from __future__ import annotations

import shutil
import subprocess


def _cli() -> str:
    dirsql_checks = shutil.which("dirsql-checks")
    assert dirsql_checks is not None, (
        "`dirsql-checks` console script not on PATH -- run "
        "`uv run --project internals/checks pytest tests/e2e` "
        "or `uv sync --project internals/checks`"
    )
    return dirsql_checks


def _package(root, dependencies, module):
    root.mkdir(parents=True, exist_ok=True)
    root.joinpath("pyproject.toml").write_text(
        f'[project]\nname = "fixture"\nversion = "0"\ndependencies = {dependencies!r}\n'
    )
    source = root / "src" / "fixture"
    source.mkdir(parents=True)
    source.joinpath("__init__.py").write_text("")
    source.joinpath("main.py").write_text(module)
    return str(source)


def _invoke(source):
    return subprocess.run(
        [_cli(), "declared-deps", source],
        capture_output=True,
        text=True,
        timeout=30,
    )


def describe_dirsql_checks_declared_deps():
    def it_exits_nonzero_and_names_the_undeclared_import(tmp_path):
        source = _package(tmp_path / "pkg", [], "from bin_shim import main\n")

        proc = _invoke(source)

        assert proc.returncode == 1, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "main.py: bin_shim" in proc.stderr

    def it_exits_zero_once_the_dependency_is_declared(tmp_path):
        source = _package(tmp_path / "pkg", ["bin-shim>=0.1"], "from bin_shim import main\n")

        proc = _invoke(source)

        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
