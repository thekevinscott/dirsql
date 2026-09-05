"""Colocated unit tests for the declared-deps orchestration (#782)."""

from checks.declared_deps.run import run


def drive(sources, dependencies=(), echo=None):
    return run(
        "pkg/src",
        manifest=lambda _path: {"project": {"dependencies": list(dependencies)}},
        distributions=lambda: {},
        read=lambda path: sources[path],
        files=lambda _source: list(sources),
        ours=lambda _source: set(),
        echo=echo or (lambda _line: None),
    )


def describe_run():
    def it_returns_zero_when_every_import_is_declared():
        assert drive({"pkg/src/m.py": "import click\n"}, dependencies=["click"]) == 0

    def it_returns_one_and_names_the_file_module_and_the_fix():
        lines = []
        code = drive({"pkg/src/m.py": "from bin_shim import main\n"}, echo=lines.append)
        assert code == 1
        assert lines[0] == "undeclared dependency -- pkg/src/m.py: bin_shim"
        assert "1 import(s) not declared in ./pyproject.toml" in lines[1]
        assert "never `uv pip install`" in lines[1]

    def it_stays_quiet_when_there_is_nothing_to_report():
        lines = []
        drive({"pkg/src/m.py": "import os\n"}, echo=lines.append)
        assert lines == []

    def it_takes_source_by_keyword():
        # `*` (not `/`) before the injected seams keeps `source` nameable.
        assert run(
            source="pkg/src",
            manifest=lambda _path: {},
            distributions=lambda: {},
            read=lambda _path: "",
            files=lambda _source: [],
            ours=lambda _source: set(),
            echo=lambda _line: None,
        ) == 0

    def it_reads_the_manifest_beside_the_derived_package_root():
        seen = []
        run(
            "pkg/src",
            manifest=lambda path: seen.append(path) or {},
            distributions=lambda: {},
            read=lambda _path: "",
            files=lambda _source: [],
            ours=lambda _source: set(),
            echo=lambda _line: None,
        )
        assert seen == ["./pyproject.toml"]
