"""Colocated unit tests for reading one `Platform(...)` entry (isolation -- an
`ast` node in, a dict out; nothing here touches the repo's real platforms.py).
"""

import ast

import pytest

from checks.platforms_mirror.row import ParseError, row

FIELDS = ["node_platform", "node_arch", "slug"]


def entry(source: str) -> ast.expr:
    """A table entry, as `ast` hands it over from the PLATFORMS assignment."""
    return ast.parse(source, mode="eval").body


def describe_row():
    def it_names_every_positional_after_the_field_in_that_position():
        assert row(entry('Platform("linux", "x64", "linux-x64-gnu")'), FIELDS) == {
            "node_platform": "linux",
            "node_arch": "x64",
            "slug": "linux-x64-gnu",
        }

    def it_lets_keywords_override_positionals():
        source = 'Platform("win32", "ia32", slug="win32-x64-msvc", node_arch="x64")'
        assert row(entry(source), FIELDS) == {
            "node_platform": "win32",
            "node_arch": "x64",
            "slug": "win32-x64-msvc",
        }

    def it_reads_a_list_valued_keyword():
        assert row(entry('Platform(os=["linux", "android"])'), ["os"]) == {
            "os": ["linux", "android"]
        }

    def it_reads_a_row_that_fills_fewer_fields_than_the_class_declares():
        assert row(entry('Platform("linux")'), FIELDS) == {"node_platform": "linux"}

    @pytest.mark.parametrize(
        "source",
        ["1", "Machine('a')", "Target('a')", "platforms.Platform('a')"],
        ids=["not-a-call", "machine", "target", "dotted"],
    )
    def it_rejects_an_entry_that_is_not_a_literal_platform_call(source):
        with pytest.raises(ParseError, match="every PLATFORMS entry must be a literal `Platform"):
            row(entry(source), FIELDS)

    def it_counts_the_positionals_and_the_fields_when_there_are_too_many():
        with pytest.raises(
            ParseError, match="passes 2 positional arguments but Platform declares 1 fields"
        ):
            row(entry('Platform("a", "b")'), ["slug"])

    def it_rejects_a_row_that_splats_keywords():
        with pytest.raises(ParseError, match="splats"):
            row(entry("Platform(**other)"), ["slug"])

    def it_rejects_a_non_literal_positional():
        with pytest.raises(ParseError, match="not a literal"):
            row(entry("Platform(slug)"), ["slug"])

    def it_rejects_a_non_literal_keyword_value():
        with pytest.raises(ParseError, match="not a literal"):
            row(entry("Platform(slug=name)"), ["slug"])
