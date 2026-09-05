"""Colocated unit tests for reading one `Platform(...)` argument (isolation --
an `ast` node in, a value out).
"""

import ast

import pytest

from checks.platforms_mirror.literal import ParseError, literal


def value(source: str) -> ast.expr:
    return ast.parse(source, mode="eval").body


def describe_literal():
    def it_reads_a_string():
        assert literal(value("'linux-x64-gnu'")) == "linux-x64-gnu"

    def it_reads_a_list_of_strings():
        assert literal(value("['linux', 'darwin']")) == ["linux", "darwin"]

    def it_rejects_a_name():
        with pytest.raises(ParseError, match="a Platform\\(...\\) argument is not a literal"):
            literal(value("slug"))

    def it_carries_the_reason_ast_gave():
        with pytest.raises(ParseError, match="malformed node or string"):
            literal(value("slug"))
