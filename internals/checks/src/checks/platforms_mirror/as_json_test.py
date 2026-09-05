"""Colocated unit tests for the JSON normalizer (isolation -- pure text in,
text out).
"""

from checks.platforms_mirror.as_json import as_json


def describe_as_json():
    def it_quotes_a_bare_object_key():
        assert as_json('{nodePlatform: "linux"}') == '{"nodePlatform": "linux"}'

    def it_quotes_every_key_after_a_comma():
        assert as_json('{a: 1, b: 2}') == '{"a": 1, "b": 2}'

    def it_quotes_a_key_holding_a_dollar_sign_or_underscore():
        assert as_json("{_a$b: 1}") == '{"_a$b": 1}'

    def it_drops_a_trailing_comma_before_a_brace():
        assert as_json('{"a": 1,}') == '{"a": 1}'

    def it_drops_a_trailing_comma_before_a_bracket():
        assert as_json('["a",]') == '["a"]'

    def it_keeps_the_whitespace_that_followed_a_dropped_comma():
        assert as_json('{"a": 1,\n}') == '{"a": 1\n}'

    def it_leaves_an_already_quoted_key_alone():
        assert as_json('{"a": 1}') == '{"a": 1}'
