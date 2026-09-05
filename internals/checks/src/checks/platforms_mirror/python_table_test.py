"""Colocated unit tests for the Python-table reader (isolation -- pure text in,
data out; nothing here touches the repo's real platforms.py).
"""

from checks.platforms_mirror.python_table import python_table

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

    def it_names_each_positional_after_the_field_in_that_position():
        # The field list is what turns a row's positionals into names, so the
        # two readers have to be composed in that order.
        _, rows = python_table(PYTHON)
        assert rows[1] == {
            "node_platform": "win32",
            "node_arch": "x64",
            "slug": "win32-x64-msvc",
        }
