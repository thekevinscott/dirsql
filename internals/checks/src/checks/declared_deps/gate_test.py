"""Colocated unit tests for the declared-deps gate (#782)."""

from checks.declared_deps.gate import (
    declared,
    first_party,
    normalize,
    package_root,
    providers,
    read_manifest,
    read_text,
    requirement_name,
    run,
    source_files,
    top_level_imports,
    undeclared,
    warn,
)

MANIFEST = {
    "project": {"dependencies": ["click>=8.1", "PyYAML==6.0"]},
    "dependency-groups": {"dev": ["pytest>=8"]},
}


def describe_normalize():
    def it_lowercases_and_folds_underscores_to_dashes():
        assert normalize("Bin_Shim") == "bin-shim"


def describe_requirement_name():
    def it_strips_the_version_specifier():
        assert requirement_name("click>=8.1") == "click"

    def it_strips_extras_markers_and_exclusions():
        assert [requirement_name(s) for s in ("a[x]", "b!=1", "c;python<4", "d ==1", "e~=2")] == [
            *["a", "b", "c", "d", "e"]
        ]


def describe_top_level_imports():
    def it_reads_plain_and_dotted_imports():
        assert top_level_imports("import os.path\nimport click\n") == {"os", "click"}

    def it_reads_from_imports_by_their_root_module():
        assert top_level_imports("from bin_shim.core import main\n") == {"bin_shim"}

    def it_ignores_relative_imports_which_are_always_first_party():
        assert top_level_imports("from . import sibling\nfrom .a.b import c\n") == set()

    def it_ignores_a_bare_relative_import_with_no_module():
        assert top_level_imports("from .. import x\n") == set()


def describe_declared():
    def it_splits_runtime_from_dev_and_normalizes_both():
        assert declared(MANIFEST) == ({"click", "pyyaml"}, {"pytest"})

    def it_returns_empty_sets_for_a_manifest_declaring_nothing():
        assert declared({}) == (set(), set())


def describe_providers():
    def it_maps_an_import_name_to_its_distributions():
        assert providers("yaml", {"yaml": ["PyYAML"]}) == {"pyyaml"}

    def it_falls_back_to_the_import_name_when_nothing_provides_it():
        assert providers("bin_shim", {}) == {"bin-shim"}


def describe_source_files():
    def it_collects_python_files_recursively_and_sorted():
        walk = lambda _root: [("src", [], ["b.py", "a.py", "notes.md"]), ("src/sub", [], ["c.py"])]  # noqa: E731
        assert source_files("src", walk) == ["src/a.py", "src/b.py", "src/sub/c.py"]


def describe_package_root():
    def it_walks_up_to_the_nearest_pyproject():
        assert package_root("a/b/c", lambda p: p == "a/pyproject.toml") == "a"

    def it_falls_back_to_the_repo_root_when_no_ancestor_has_one():
        assert package_root("a/b", lambda _p: False) == "."


def describe_first_party():
    def it_names_the_scanned_dir_and_everything_directly_inside_it():
        assert first_party("src/checks", lambda _d: ["changelog.py", "preflight"]) == {
            *["checks", "changelog", "preflight"]
        }

    def it_ignores_a_trailing_slash_on_the_scanned_dir():
        assert "checks" in first_party("src/checks/", lambda _d: [])


def check(sources, manifest=MANIFEST, distributions=None, ours=frozenset()):
    return undeclared(
        "src",
        manifest,
        distributions or {},
        lambda path: sources[path],
        list(sources),
        set(ours),
    )


def describe_undeclared():
    def it_reports_an_import_no_declared_distribution_provides():
        assert check({"src/main.py": "from bin_shim import main\n"}) == [
            "src/main.py: bin_shim"
        ]

    def it_accepts_a_declared_runtime_dependency():
        assert check({"src/main.py": "import click\n"}) == []

    def it_accepts_a_declared_dependency_reached_under_a_different_import_name():
        assert check({"src/m.py": "import yaml\n"}, distributions={"yaml": ["PyYAML"]}) == []

    def it_accepts_the_standard_library():
        assert check({"src/main.py": "import os\nimport tomllib\n"}) == []

    def it_keeps_scanning_a_file_after_an_accepted_import():
        # `ast` sorts before `bin_shim`, so a `break` on the skip would hide it.
        assert check({"src/main.py": "import ast\nfrom bin_shim import x\n"}) == [
            "src/main.py: bin_shim"
        ]

    def it_accepts_a_first_party_module():
        assert check({"src/main.py": "import checks\n"}, ours={"checks"}) == []

    def it_accepts_a_dev_dependency_in_a_colocated_test():
        assert check({"src/main_test.py": "import pytest\n"}) == []

    def it_accepts_a_dependency_declared_in_both_groups_from_a_test():
        # `runtime | dev`, not `^`: a name in both lists is still allowed.
        both = {
            "project": {"dependencies": ["click"]},
            "dependency-groups": {"dev": ["click"]},
        }
        assert check({"src/main_test.py": "import click\n"}, manifest=both) == []

    def it_rejects_a_dev_dependency_in_shipped_source():
        assert check({"src/main.py": "import pytest\n"}) == ["src/main.py: pytest"]

    def it_reports_every_offending_import_in_file_then_module_order():
        assert check({"src/b.py": "import zz\nimport aa\n", "src/a.py": "import q\n"}) == [
            *["src/b.py: aa", "src/b.py: zz", "src/a.py: q"]
        ]


def describe_read_text():
    def it_reads_a_file_as_utf8(tmp_path):
        path = tmp_path / "s.py"
        path.write_text("# héllo\n", encoding="utf-8")
        assert read_text(str(path)) == "# héllo\n"


def describe_read_manifest():
    def it_parses_a_toml_manifest(tmp_path):
        path = tmp_path / "pyproject.toml"
        path.write_text('[project]\nname = "x"\n')
        assert read_manifest(str(path)) == {"project": {"name": "x"}}


def describe_warn():
    def it_writes_to_stderr(capsys):
        warn("boom")
        assert capsys.readouterr().err == "boom\n"


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
