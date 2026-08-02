"""Unit tests for `user_passed_config`."""

from .user_passed_config import user_passed_config


def describe_user_passed_config():
    def it_is_true_for_the_bare_short_flag():
        assert user_passed_config(["-c", "x.toml"]) is True

    def it_is_true_for_the_attached_short_value():
        assert user_passed_config(["-c./x.toml"]) is True

    def it_is_true_for_the_long_flag():
        assert user_passed_config(["--config", "x.toml"]) is True

    def it_is_true_for_the_config_equals_form():
        assert user_passed_config(["--config=x.toml"]) is True

    def it_is_false_without_any_config_flag():
        assert user_passed_config(["query", "SELECT 1"]) is False

    def it_does_not_mistake_a_long_lookalike_for_a_short_flag():
        # `--configx` is not `--config`, not `--config=…`, and does not start
        # with `-c` (it starts with `--`) -- so no clause matches.
        assert user_passed_config(["--configx"]) is False

    def it_is_false_for_a_flag_that_sorts_before_config():
        # `--all` sorts lexically before `--config` but is not a config flag;
        # pins the `==` comparison against a `<=` mutant.
        assert user_passed_config(["--all"]) is False
