"""Unit tests for `with_resolved_extensions`.

The shared SDK resolver (mocked here) owns the TOML parsing and package-name
gating; this file covers only the launcher-side argv plumbing: config-path
extraction, the `init` / native-config guards, and `--extension` flag
construction.
"""

from unittest import mock

import dirsql_cli.resolve_config_extensions as rce


def _patch(specs):
    return mock.patch.object(rce, "resolve_config_extension_specs", return_value=specs)


def describe_with_resolved_extensions():
    def it_passes_init_through_untouched():
        argv = ["init", "--root", "."]
        with _patch([{"path": "R:pkg", "entrypoint": None}]) as resolver:
            assert rce.with_resolved_extensions(argv) is argv
            resolver.assert_not_called()

    def it_passes_a_native_config_through_untouched():
        argv = ["--config", "dirsql.config.py"]
        with _patch([{"path": "R:pkg", "entrypoint": None}]) as resolver:
            assert rce.with_resolved_extensions(argv) is argv
            resolver.assert_not_called()

    def it_passes_through_when_the_resolver_does_not_intervene():
        argv = ["--config", "/x/.dirsql.toml"]
        with _patch(None) as resolver:
            assert rce.with_resolved_extensions(argv) is argv
        resolver.assert_called_once_with("/x/.dirsql.toml")

    def it_appends_extension_flags_for_resolved_specs():
        specs = [
            {"path": "R:sqlite_vec", "entrypoint": "sqlite3_vec_init"},
            {"path": "R:ext/local.so", "entrypoint": None},
        ]
        with _patch(specs):
            out = rce.with_resolved_extensions(["--config", "/cfg/.dirsql.toml"])
        assert out == [
            "--config",
            "/cfg/.dirsql.toml",
            "--extension",
            "R:sqlite_vec::sqlite3_vec_init",
            "--extension",
            "R:ext/local.so",
        ]

    def it_reads_the_config_equals_form():
        with _patch([{"path": "R:pkg", "entrypoint": None}]) as resolver:
            out = rce.with_resolved_extensions(["--config=/c/.dirsql.toml"])
        assert out == ["--config=/c/.dirsql.toml", "--extension", "R:pkg"]
        resolver.assert_called_once_with("/c/.dirsql.toml")

    def it_defaults_to_dot_dirsql_toml_when_no_config_given():
        with _patch([{"path": "R:pkg", "entrypoint": None}]) as resolver:
            out = rce.with_resolved_extensions(["--port", "9000"])
        assert out == ["--port", "9000", "--extension", "R:pkg"]
        resolver.assert_called_once_with("./.dirsql.toml")

    def it_treats_a_bare_trailing_config_as_empty_path():
        with _patch(None) as resolver:
            argv = ["--config"]
            assert rce.with_resolved_extensions(argv) is argv
        resolver.assert_called_once_with("")

    def it_reads_the_config_value_at_any_argv_position():
        with _patch(None) as resolver:
            rce.with_resolved_extensions(["-v", "--config", "/x/y", "tail"])
        resolver.assert_called_once_with("/x/y")

    def it_matches_the_config_flag_by_value_not_identity_or_ordering():
        # A runtime-built "--config" (not the interned literal) must match,
        # and flags sorting on either side of "--config" must not.
        with _patch(None) as resolver:
            rce.with_resolved_extensions(["".join(["--con", "fig"]), "/x/y"])
        resolver.assert_called_once_with("/x/y")
        with _patch(None) as resolver:
            rce.with_resolved_extensions(["--a", "val"])
        resolver.assert_called_once_with("./.dirsql.toml")

    def it_matches_init_by_value_not_identity_or_ordering():
        # Only the exact first argument "init" skips resolution: a
        # runtime-built "init" still skips; other subcommands (sorting above
        # or below "init") do not.
        with _patch(None) as resolver:
            argv = ["".join(["in", "it"])]
            assert rce.with_resolved_extensions(argv) is argv
            resolver.assert_not_called()
        with _patch(None) as resolver:
            rce.with_resolved_extensions(["zzz"])
        resolver.assert_called_once_with("./.dirsql.toml")

    def it_consults_the_resolver_for_an_empty_argv():
        with _patch(None) as resolver:
            argv: list[str] = []
            assert rce.with_resolved_extensions(argv) is argv
        resolver.assert_called_once_with("./.dirsql.toml")
