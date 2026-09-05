"""Unit tests for `config_paths_from_argv`."""

from dirsql.cli.config_paths_from_argv import config_paths_from_argv


def describe_config_paths_from_argv():
    def it_reads_the_config_form():
        assert config_paths_from_argv(["--config", "/cfg/.dirsql.toml"]) == [
            "/cfg/.dirsql.toml"
        ]

    def it_reads_the_config_equals_form():
        assert config_paths_from_argv(["--config=/c/.dirsql.toml"]) == [
            "/c/.dirsql.toml"
        ]

    def it_reads_the_short_c_form():
        # Discovery injects fragments as `-c <path>`; those must be collected
        # exactly like `--config <path>`.
        assert config_paths_from_argv(["-c", "/frag/dirsql.toml"]) == [
            "/frag/dirsql.toml"
        ]

    def it_reads_the_short_c_equals_form():
        assert config_paths_from_argv(["-c=/frag/dirsql.toml"]) == ["/frag/dirsql.toml"]

    def it_reads_the_short_c_attached_form():
        assert config_paths_from_argv(["-c/frag/dirsql.toml"]) == ["/frag/dirsql.toml"]

    def it_collects_every_config_flag_in_argv_order():
        argv = ["-c", "a.toml", "--config", "b.toml", "--config=c.toml", "-cd.toml"]
        assert config_paths_from_argv(argv) == ["a.toml", "b.toml", "c.toml", "d.toml"]

    def it_reads_the_config_value_at_any_argv_position():
        assert config_paths_from_argv(["-v", "--config", "/x/y", "tail"]) == ["/x/y"]

    def it_collects_a_discovery_injected_fragment_not_the_default():
        # The user passed no config; discovery appended `--include-default -c
        # <fragment>`. The fragment -- not `./.dirsql.toml` -- is the result.
        argv = ["query", "SELECT 1", "--include-default", "-c", "/frag/dirsql.toml"]
        assert config_paths_from_argv(argv) == ["/frag/dirsql.toml"]

    def it_consumes_a_config_value_that_looks_like_a_flag():
        # The token after `--config` / `-c` is that flag's value; it is never
        # re-parsed as another config flag.
        assert config_paths_from_argv(["--config", "-cx.toml"]) == ["-cx.toml"]

    def it_matches_the_config_flag_by_value_not_identity_or_ordering():
        # A runtime-built "--config" (not the interned literal) must match,
        # and flags sorting on either side of "--config" must not.
        assert config_paths_from_argv(["".join(["--con", "fig"]), "/x/y"]) == ["/x/y"]
        assert config_paths_from_argv(["--a", "val"]) == ["./.dirsql.toml"]

    def it_does_not_mistake_other_short_flags_for_config():
        assert config_paths_from_argv(["-v", "-x", "val"]) == ["./.dirsql.toml"]

    def it_defaults_to_dot_dirsql_toml_when_no_config_given():
        assert config_paths_from_argv(["--port", "9000"]) == ["./.dirsql.toml"]

    def it_defaults_to_dot_dirsql_toml_for_an_empty_argv():
        assert config_paths_from_argv([]) == ["./.dirsql.toml"]

    def it_treats_a_bare_trailing_config_as_an_empty_path():
        assert config_paths_from_argv(["--config"]) == [""]

    def it_treats_a_bare_trailing_short_c_as_an_empty_path():
        assert config_paths_from_argv(["-c"]) == [""]

    def it_keeps_a_bare_trailing_short_c_after_an_earlier_config():
        assert config_paths_from_argv(["--config", "a.toml", "-c"]) == ["a.toml", ""]
