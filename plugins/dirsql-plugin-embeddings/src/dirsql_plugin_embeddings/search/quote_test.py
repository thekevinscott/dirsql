from . import quote as module


def describe_quote():
    def it_wraps_text_in_single_quotes():
        assert module.quote("hello") == "'hello'"

    def it_doubles_embedded_single_quotes():
        assert module.quote("O'Brien") == "'O''Brien'"

    def it_keeps_an_injection_shaped_value_inside_the_literal():
        assert module.quote("'; DROP TABLE x; --") == "'''; DROP TABLE x; --'"
