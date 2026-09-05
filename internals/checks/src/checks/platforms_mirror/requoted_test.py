"""Colocated unit tests for the string re-quoter (isolation -- pure text in,
text out).
"""

from checks.platforms_mirror.requoted import requoted


def describe_requoted():
    def it_keeps_a_double_quoted_literal_verbatim():
        assert requoted('"linux"') == '"linux"'

    def it_rewrites_a_single_quoted_literal_as_json():
        assert requoted("'linux'") == '"linux"'

    def it_rewrites_a_template_literal_as_json():
        assert requoted("`linux`") == '"linux"'

    def it_keeps_the_contents_of_a_single_quoted_literal_whole():
        assert requoted("'aba'") == '"aba"'

    def it_unescapes_an_escaped_single_quote():
        assert requoted(r"'it\'s'") == '"it\'s"'

    def it_unescapes_an_escaped_double_quote_inside_a_template():
        assert requoted(r"`a\"b`") == '"a\\"b"'

    def it_escapes_a_double_quote_that_was_bare_in_a_single_quoted_literal():
        assert requoted("'a\"b'") == '"a\\"b"'
