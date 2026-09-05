"""Colocated unit tests for stripping comments and normalizing strings
(isolation -- source text in, source text out).
"""

from checks.platforms_mirror.without_comments import without_comments


def describe_without_comments():
    def it_drops_a_line_comment():
        assert without_comments("a // gone\nb") == "a \nb"

    def it_drops_a_block_comment_across_lines():
        assert without_comments("a /* gone\ngone */ b") == "a  b"

    def it_keeps_a_comment_marker_that_sits_inside_a_string():
        assert without_comments("['a // b']") == '["a // b"]'

    def it_normalizes_the_strings_it_keeps():
        assert without_comments("['linux']") == '["linux"]'

    def it_drops_a_line_comment_that_runs_to_the_end_of_the_source():
        assert without_comments("a // gone") == "a "

    def it_leaves_a_source_with_neither_alone():
        assert without_comments("[1, 2]") == "[1, 2]"
