"""Colocated unit tests for normalizing an object literal to JSON (isolation --
source text in, source text out).
"""

from checks.platforms_mirror.as_json import as_json


def describe_as_json():
    def it_quotes_a_bare_key():
        assert as_json('{os: "linux"}') == '{"os": "linux"}'

    def it_quotes_every_key_of_an_object():
        assert as_json('{os: "linux", cpu: "x64"}') == '{"os": "linux", "cpu": "x64"}'

    def it_leaves_an_already_quoted_key_alone():
        assert as_json('{"os": "linux"}') == '{"os": "linux"}'

    def it_drops_a_trailing_comma_before_a_brace():
        assert as_json('{"os": "linux",}') == '{"os": "linux"}'

    def it_drops_a_trailing_comma_before_a_bracket():
        assert as_json('["linux",]') == '["linux"]'
