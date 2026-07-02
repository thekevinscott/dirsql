"""Unit tests for `with_resolved_extensions`.

The shared SDK resolver (`dirsql.resolve_config_extensions`, mocked here) owns
the TOML parsing and package-name gating and carries its own colocated tests;
this file covers only the launcher-side argv plumbing: config-path extraction,
the `init` / native-config guards, and `--extension` flag construction.
"""

from unittest import mock

import dirsql.cli.resolve_config_extensions as rce


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
