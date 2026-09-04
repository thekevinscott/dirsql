"""Colocated unit tests for the shared platform-table vocabulary (#1004)."""

from checks.platforms_mirror.parse import CLASS_NAME, ParseError, TABLE_NAME


def describe_names():
    def it_names_the_dataclass_the_python_table_declares():
        assert CLASS_NAME == "Platform"

    def it_names_the_binding_both_sides_hold_the_table_under():
        assert TABLE_NAME == "PLATFORMS"


def describe_ParseError():
    def it_carries_the_message_a_reader_raised_it_with():
        assert str(ParseError("no `class Platform`")) == "no `class Platform`"

    def it_is_an_ordinary_exception():
        assert issubclass(ParseError, Exception)
