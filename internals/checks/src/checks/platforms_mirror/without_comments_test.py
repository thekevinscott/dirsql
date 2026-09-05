"""Colocated unit tests for the comment stripper (isolation -- pure text in,
text out; nothing here touches the repo's real platforms.ts).
"""

from checks.platforms_mirror.without_comments import without_comments


def describe_without_comments():
    def it_strips_a_line_comment():
        assert without_comments("a // gone\nb") == "a \nb"

    def it_strips_a_line_comment_that_runs_to_end_of_file():
        assert without_comments("a // gone") == "a "

    def it_strips_a_block_comment():
        assert without_comments("a /* gone */ b") == "a  b"

    def it_strips_a_block_comment_spanning_lines():
        assert without_comments("a /* one\ntwo */ b") == "a  b"

    def it_keeps_a_double_quoted_string_verbatim():
        assert without_comments('x = "linux"') == 'x = "linux"'

    def it_requotes_a_single_quoted_string():
        assert without_comments("x = 'linux'") == 'x = "linux"'

    def it_requotes_a_template_literal():
        assert without_comments("x = `linux`") == 'x = "linux"'

    def it_does_not_read_a_line_comment_marker_inside_a_string():
        assert without_comments('x = "a // b"') == 'x = "a // b"'

    def it_does_not_read_a_block_comment_marker_inside_a_string():
        assert without_comments("x = 'a /* b'") == 'x = "a /* b"'

    def it_keeps_an_escaped_quote_inside_a_string():
        assert without_comments(r'x = "a\"b"') == r'x = "a\"b"'

    def it_leaves_source_with_neither_comment_nor_string_alone():
        assert without_comments("[1, 2]") == "[1, 2]"
