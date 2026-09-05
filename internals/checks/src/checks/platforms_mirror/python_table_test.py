"""Colocated unit tests for the Python-table reader (isolation -- pure text in,
data out; nothing here touches the repo's real platforms.py).
"""

import pytest

from checks.platforms_mirror.python_table import ParseError, python_table

PYTHON = '''\
from dataclasses import dataclass


@dataclass(frozen=True)
class Platform:
    node_platform: str
    node_arch: str
    slug: str


PLATFORMS: tuple[Platform, ...] = (
    Platform("linux", "x64", "linux-x64-gnu"),
    Platform("win32", "x64", slug="win32-x64-msvc", node_arch="x64"),
)
'''


def describe_python_table():
    def it_reads_the_annotated_fields_and_the_rows():
        fields, rows = python_table(PYTHON)
        assert fields == ["node_platform", "node_arch", "slug"]
        assert rows[0] == {"node_platform": "linux", "node_arch": "x64", "slug": "linux-x64-gnu"}

    def it_reads_every_row_of_the_table():
        assert len(python_table(PYTHON)[1]) == 2

    def it_reads_a_plain_assignment_without_an_annotation():
        source = PYTHON.replace("PLATFORMS: tuple[Platform, ...] =", "PLATFORMS =")
        assert len(python_table(source)[1]) == 2

    @pytest.mark.parametrize("name", ["Machine", "Target"])
    def it_rejects_a_module_whose_only_class_is_named_otherwise(name):
        with pytest.raises(ParseError, match="no `class Platform`"):
            python_table(f"class {name}:\n    slug: str\n\n\nPLATFORMS = ()\n")

    def it_rejects_a_module_with_no_class_at_all():
        with pytest.raises(ParseError, match="no `class Platform`"):
            python_table("PLATFORMS = ()\n")

    def it_rejects_a_platform_class_with_no_annotated_fields():
        with pytest.raises(ParseError, match="declares no annotated fields"):
            python_table("class Platform:\n    pass\n\n\nPLATFORMS = ()\n")

    @pytest.mark.parametrize("name", ["OTHERS", "TARGETS"])
    def it_rejects_a_table_bound_under_another_name(name):
        with pytest.raises(ParseError, match="no module-level"):
            python_table(f"class Platform:\n    slug: str\n\n\n{name} = ()\n")

    def it_rejects_a_module_with_no_table():
        with pytest.raises(ParseError, match="no module-level"):
            python_table("class Platform:\n    slug: str\n")

    def it_reads_a_table_bound_alongside_another_name():
        source = "class Platform:\n    slug: str\n\n\nOTHERS = PLATFORMS = (Platform('a'),)\n"
        assert python_table(source)[1] == [{"slug": "a"}]

    def it_rejects_a_computed_table():
        source = PYTHON.replace("PLATFORMS: tuple[Platform, ...] = (", "PLATFORMS = tuple(")
        with pytest.raises(ParseError, match="not a tuple or list literal"):
            python_table(source.replace(")\n", ")\n", 1))

    def it_surfaces_a_row_it_cannot_read():
        with pytest.raises(ParseError, match="literal `Platform"):
            python_table("class Platform:\n    slug: str\n\n\nPLATFORMS = (1,)\n")
