"""Colocated unit tests for the declared-deps verdict (#782).

Isolation: the source reader is injected. `declared`, `top_level_imports` and
`providers` run for real -- each is a pure text-in / names-out helper.
"""

from checks.declared_deps.undeclared import undeclared

MANIFEST = {
    "project": {"dependencies": ["click>=8.1", "PyYAML==6.0"]},
    "dependency-groups": {"dev": ["pytest>=8"]},
}


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
