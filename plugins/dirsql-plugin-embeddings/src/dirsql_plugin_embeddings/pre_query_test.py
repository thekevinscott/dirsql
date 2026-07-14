"""Colocated unit tests for the pre-query console script (isolation).

The embedder is mocked; stdout is captured. No real network.
"""

from unittest import mock

import pytest

from . import pre_query as module
from .pre_query import build_sql, main, question


def describe_question():
    def it_reads_a_verbatim_server_body():
        assert question('{"q": "cook pasta"}') == "cook pasta"

    def it_unwraps_the_cli_sql_wrapper():
        assert question('{"sql": "{\\"q\\": \\"cook pasta\\"}"}') == "cook pasta"

    def it_prefers_q_when_both_keys_are_present():
        assert question('{"q": "outer", "sql": "{\\"q\\": \\"inner\\"}"}') == "outer"

    def it_raises_when_no_question_is_present():
        with pytest.raises(KeyError):
            question('{"other": 1}')


def describe_build_sql():
    def it_builds_nearest_neighbor_sql_for_the_vector():
        assert build_sql([1.0, 2.0]) == (
            "SELECT path, ROUND(vec_distance_cosine(embedding, '[1.0, 2.0]'), 3) "
            "AS distance FROM documents ORDER BY distance LIMIT 3"
        )


def describe_main():
    def it_embeds_the_question_and_prints_the_sql(capsys):
        with mock.patch.object(module, "embed", return_value=[0.1, 0.2]) as embed:
            assert main(["prog", '{"q": "how do I cook pasta?"}']) == 0
        embed.assert_called_once_with("how do I cook pasta?")
        out = capsys.readouterr().out
        assert out == build_sql([0.1, 0.2]) + "\n"

    def it_defaults_to_sys_argv(capsys):
        with (
            mock.patch.object(module.sys, "argv", ["prog", '{"q": "hi"}']),
            mock.patch.object(module, "embed", return_value=[9.0]) as embed,
        ):
            assert main() == 0
        embed.assert_called_once_with("hi")
        assert capsys.readouterr().out == build_sql([9.0]) + "\n"
