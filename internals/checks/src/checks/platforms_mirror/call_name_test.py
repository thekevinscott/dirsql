"""Colocated unit tests for reading a table entry's call name (isolation -- an
`ast` node in, a name out).
"""

import ast

from checks.platforms_mirror.call_name import call_name


def entry(source: str) -> ast.expr:
    return ast.parse(source, mode="eval").body


def describe_call_name():
    def it_names_a_plain_call():
        assert call_name(entry("Platform('linux')")) == "Platform"

    def it_names_the_callee_rather_than_its_arguments():
        assert call_name(entry("Machine(Platform('linux'))")) == "Machine"

    def it_has_no_name_for_a_dotted_call():
        assert call_name(entry("platforms.Platform('linux')")) is None

    def it_has_no_name_for_something_that_is_not_a_call():
        assert call_name(entry("1")) is None
