from unittest.mock import MagicMock, patch

from . import main


def describe_main():
    def it_is_a_group_with_exactly_the_worker_and_search_subcommands():
        assert set(main.main.commands) == {"worker", "search"}

    def it_registers_the_worker_command_module():
        assert main.main.commands["worker"] is main.worker

    def it_registers_the_search_command_module():
        assert main.main.commands["search"] is main.search

    def it_uses_the_default_command_group():
        assert isinstance(main.main, main.DefaultCommandGroup)


def describe_default_command_routing():
    def _parsed(args):
        with patch.object(
            main.click.Group, "parse_args", return_value="parsed"
        ) as base:
            result = main.main.parse_args(MagicMock(), args)
        assert result == "parsed"
        return base.call_args.args[1]

    def it_inserts_search_for_bare_positionals():
        assert _parsed(["g/**", "hello"]) == ["search", "g/**", "hello"]

    def it_inserts_search_when_there_are_no_arguments():
        assert _parsed([]) == ["search"]

    def it_inserts_search_for_a_leading_option():
        assert _parsed(["-k", "3", "g", "q"]) == ["search", "-k", "3", "g", "q"]

    def it_leaves_the_visible_worker_subcommand_alone():
        assert _parsed(["worker"]) == ["worker"]

    def it_treats_a_literal_search_token_as_a_glob():
        assert _parsed(["search", "x"]) == ["search", "search", "x"]

    def it_leaves_the_group_help_flag_alone():
        assert _parsed(["--help"]) == ["--help"]

    def it_compares_the_first_token_for_equality_not_ordering():
        assert _parsed(["!sorts-before---help", "q"]) == [
            "search",
            "!sorts-before---help",
            "q",
        ]

    def it_routes_on_the_first_token_even_when_help_comes_later():
        assert _parsed(["g", "--help"]) == ["search", "g", "--help"]

    def it_routes_on_the_first_token_even_when_a_command_name_comes_later():
        assert _parsed(["g/**", "worker"]) == ["search", "g/**", "worker"]
