"""Unit tests for `with_resolved_extensions`.

Both collaborators are mocked: the argv scan (`config_paths_from_argv`) and
the shared SDK resolver, which owns the TOML parsing and package-name gating.
What is left is the launcher's own plumbing -- the `init` and native-config
guards, and `--extension` flag construction.
"""

from contextlib import contextmanager
from unittest import mock

import dirsql.cli.resolve_config_extensions as rce


@contextmanager
def _patch(specs, paths=("/x/.dirsql.toml",)):
    with (
        mock.patch.object(
            rce, "config_paths_from_argv", autospec=True, return_value=list(paths)
        ) as scan,
        mock.patch.object(
            rce, "resolve_configs_extension_specs", autospec=True, return_value=specs
        ) as resolver,
    ):
        yield scan, resolver


def describe_with_resolved_extensions():
    def it_passes_init_through_untouched():
        argv = ["init", "--root", "."]
        with _patch([{"path": "R:pkg", "entrypoint": None}]) as (scan, resolver):
            assert rce.with_resolved_extensions(argv) is argv
            scan.assert_not_called()
            resolver.assert_not_called()

    def it_matches_init_by_value_not_identity_or_ordering():
        # Only the exact first argument "init" skips resolution: a
        # runtime-built "init" still skips; other subcommands (sorting above
        # or below "init") do not.
        with _patch(None) as (_scan, resolver):
            argv = ["".join(["in", "it"])]
            assert rce.with_resolved_extensions(argv) is argv
            resolver.assert_not_called()
        with _patch(None) as (_scan, resolver):
            rce.with_resolved_extensions(["zzz"])
        resolver.assert_called_once_with(["/x/.dirsql.toml"])

    def it_scans_the_whole_argv_for_config_paths():
        argv = ["query", "SELECT 1", "--include-default", "-c", "/frag/dirsql.toml"]
        with _patch(None) as (scan, _resolver):
            rce.with_resolved_extensions(argv)
        scan.assert_called_once_with(argv)

    def it_passes_native_configs_through_untouched():
        argv = ["--config", "dirsql.config.py"]
        paths = ["cfg.py", "cfg.js", "cfg.mjs", "cfg.cjs"]
        with _patch([{"path": "R:pkg", "entrypoint": None}], paths) as (_s, resolver):
            assert rce.with_resolved_extensions(argv) is argv
            resolver.assert_not_called()

    def it_drops_native_configs_but_resolves_the_toml_ones():
        paths = ["cfg.py", "/frag/dirsql.toml", "other.cjs"]
        with _patch(None, paths) as (_scan, resolver):
            rce.with_resolved_extensions(["-c", "cfg.py", "-c", "/frag/dirsql.toml"])
        resolver.assert_called_once_with(["/frag/dirsql.toml"])

    def it_passes_through_when_the_resolver_does_not_intervene():
        argv = ["--config", "/x/.dirsql.toml"]
        with _patch(None) as (_scan, resolver):
            assert rce.with_resolved_extensions(argv) is argv
        resolver.assert_called_once_with(["/x/.dirsql.toml"])

    def it_appends_extension_flags_for_resolved_specs():
        specs = [
            {"path": "R:sqlite_vec", "entrypoint": "sqlite3_vec_init"},
            {"path": "R:ext/local.so", "entrypoint": None},
        ]
        with _patch(specs, ["/cfg/.dirsql.toml"]):
            out = rce.with_resolved_extensions(["--config", "/cfg/.dirsql.toml"])
        assert out == [
            "--config",
            "/cfg/.dirsql.toml",
            "--extension",
            "R:sqlite_vec::sqlite3_vec_init",
            "--extension",
            "R:ext/local.so",
        ]

    def it_consults_the_resolver_for_an_empty_argv():
        with _patch(None) as (scan, resolver):
            argv: list[str] = []
            assert rce.with_resolved_extensions(argv) is argv
            scan.assert_called_once_with([])
        resolver.assert_called_once_with(["/x/.dirsql.toml"])
