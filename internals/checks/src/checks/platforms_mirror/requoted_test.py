"""Colocated unit tests for re-quoting a TypeScript string literal (isolation --
one literal in, one literal out).
"""

from checks.platforms_mirror.requoted import requoted


def describe_requoted():
    def it_keeps_a_double_quoted_literal_verbatim():
        assert requoted('"linux"') == '"linux"'

    def it_rewrites_a_single_quoted_literal():
        assert requoted("'linux'") == '"linux"'

    def it_rewrites_a_template_literal():
        assert requoted("`linux`") == '"linux"'

    def it_unwraps_only_the_outermost_quotes():
        assert requoted("'aba'") == '"aba"'

    def it_unescapes_a_quote_the_typescript_form_had_to_escape():
        assert requoted(r"'a\'b'") == '"a\'b"'

    def it_keeps_an_escaped_double_quote_escaped():
        assert requoted(r"'a\"b'") == r'"a\"b"'
