"""Integration tests for the declared-deps check against real fixture packages.

Exercises `gate.run` with its default collaborators (the real filesystem, the
real installed-distribution metadata) over a real scratch package tree, never the
packaged `dirsql-checks` CLI (that's the e2e tier).

The reproduction is #777 exactly: a source file grows `from bin_shim import ...`
while `[project].dependencies` is untouched, because `uv pip install bin-shim`
populates the venv without declaring anything. Every local tier passed; seven CI
jobs went red on `error[unresolved-import]: Cannot resolve imported module`.
"""

from __future__ import annotations

from checks.declared_deps.run import run


def write_package(root, dependencies, dev=(), sources=None):
    root.mkdir(parents=True, exist_ok=True)
    root.joinpath("pyproject.toml").write_text(
        "[project]\n"
        'name = "fixture"\n'
        'version = "0"\n'
        f"dependencies = {list(dependencies)!r}\n"
        "\n[dependency-groups]\n"
        f"dev = {list(dev)!r}\n"
    )
    source = root / "src" / "fixture"
    source.mkdir(parents=True)
    source.joinpath("__init__.py").write_text("")
    for name, text in (sources or {}).items():
        source.joinpath(name).write_text(text)
    return str(source)


def describe_run_against_a_real_package():
    def it_fails_on_a_runtime_import_that_is_not_declared(tmp_path, capsys):
        source = write_package(
            tmp_path / "pkg",
            dependencies=["click>=8.1"],
            sources={"main.py": "from bin_shim import main\n"},
        )

        assert run(source) == 1
        assert "bin_shim" in capsys.readouterr().err

    def it_passes_once_the_dependency_is_declared(tmp_path):
        source = write_package(
            tmp_path / "pkg",
            dependencies=["click>=8.1", "bin-shim>=0.1"],
            sources={"main.py": "from bin_shim import main\n"},
        )

        assert run(source) == 0

    def it_accepts_the_stdlib_and_the_packages_own_modules(tmp_path):
        source = write_package(
            tmp_path / "pkg",
            dependencies=[],
            sources={
                "main.py": "import os.path\nimport tomllib\nfrom fixture import helper\n",
                "helper.py": "from . import main\n",
            },
        )

        assert run(source) == 0

    def it_allows_a_dev_group_dependency_in_a_colocated_test_only(tmp_path):
        source = write_package(
            tmp_path / "pkg",
            dependencies=[],
            dev=["pytest>=8"],
            sources={
                "main_test.py": "import pytest\n",
                "main.py": "x = 1\n",
            },
        )

        assert run(source) == 0

    def it_rejects_a_dev_group_dependency_imported_by_non_test_source(tmp_path, capsys):
        source = write_package(
            tmp_path / "pkg",
            dependencies=[],
            dev=["pytest>=8"],
            sources={"main.py": "import pytest\n"},
        )

        assert run(source) == 1
        assert "pytest" in capsys.readouterr().err
