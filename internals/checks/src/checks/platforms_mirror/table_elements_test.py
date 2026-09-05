"""Colocated unit tests for the PLATFORMS table reader (isolation -- pure text
in, AST nodes out; nothing here touches the repo's real platforms.py).
"""

import ast

import pytest

from checks.platforms_mirror.table_elements import ParseError, table_elements


def elements(source: str):
    return table_elements(ast.parse(source))


def describe_table_elements():
    def it_reads_the_rows_of_an_annotated_assignment():
        assert len(elements("PLATFORMS: tuple = (Platform('a'), Platform('b'))\n")) == 2

    def it_reads_a_plain_assignment_without_an_annotation():
        assert len(elements("PLATFORMS = (Platform('a'),)\n")) == 1

    def it_reads_a_list_literal():
        assert len(elements("PLATFORMS = [Platform('a')]\n")) == 1

    def it_reads_a_table_bound_alongside_another_name():
        assert len(elements("OTHERS = PLATFORMS = (Platform('a'),)\n")) == 1

    def it_hands_back_the_row_expressions_themselves():
        (element,) = elements("PLATFORMS = (Platform('a'),)\n")
        assert isinstance(element, ast.Call)

    @pytest.mark.parametrize("name", ["OTHERS", "TARGETS"])
    def it_rejects_a_table_bound_under_another_name(name):
        with pytest.raises(ParseError, match="no module-level"):
            elements(f"{name} = ()\n")

    def it_rejects_a_module_that_binds_no_names_at_all():
        with pytest.raises(ParseError, match="no module-level"):
            elements("import os\n")

    def it_rejects_a_computed_table():
        with pytest.raises(ParseError, match="not a tuple or list literal"):
            elements("PLATFORMS = tuple(rows)\n")

    def it_names_the_table_it_could_not_find():
        with pytest.raises(ParseError, match="`PLATFORMS = "):
            elements("OTHERS = ()\n")
