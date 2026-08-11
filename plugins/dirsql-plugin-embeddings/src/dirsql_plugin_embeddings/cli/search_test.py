from unittest.mock import MagicMock, call, patch

from . import search


def describe_search_command():
    def it_is_a_click_command_named_search():
        assert search.search.name == "search"
        assert callable(search.search.callback)

    def it_requires_glob_then_query_as_positional_arguments():
        arguments = [
            param
            for param in search.search.params
            if param.param_type_name == "argument"
        ]
        assert [param.name for param in arguments] == ["glob", "query"]
        assert all(param.required for param in arguments)

    def it_accepts_both_limit_spellings_defaulting_to_ten():
        (limit,) = [p for p in search.search.params if p.name == "limit"]
        assert set(limit.opts) == {"-k", "--limit"}
        assert limit.default == 10
        assert limit.show_default is True
        assert limit.type.name == "integer"

    def it_defaults_the_model_to_none():
        (model,) = [p for p in search.search.params if p.name == "model"]
        assert model.opts == ["--model"]
        assert model.default is None

    def it_echoes_each_line_run_search_returns():
        fake_click = MagicMock()
        with patch.object(
            search, "run_search", return_value=["a\t0.1", "b\t0.9"]
        ) as run:
            with patch.object(search, "click", fake_click):
                search.search.callback("g/**", "hello", 10, None)
        run.assert_called_once_with("g/**", "hello", 10, None)
        assert fake_click.echo.call_args_list == [
            call("a\t0.1"),
            call("b\t0.9"),
        ]

    def it_passes_limit_and_model_through_to_run_search():
        with patch.object(search, "run_search", return_value=[]) as run:
            with patch.object(search, "click", MagicMock()):
                search.search.callback("g", "q", 3, "my/model")
        run.assert_called_once_with("g", "q", 3, "my/model")
