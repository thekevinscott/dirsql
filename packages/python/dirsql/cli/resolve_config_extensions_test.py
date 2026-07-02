"""Unit tests for `with_resolved_extensions`.

Real `.dirsql.toml` files are parsed (matching the `resolve_config_test`
pattern); the only mocked collaborator is `resolve_extension_path` (the
effectful file-vs-package resolver, unit-tested in `resolve_extension_test`).
The pure `is_bare_name` stays real.
"""

import os
import tempfile
from unittest import mock

import pytest

import dirsql.cli.resolve_config_extensions as rce


@pytest.fixture
def cfg_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def _write(path, body):
    with open(path, "w") as f:
        f.write(body)


def _patch():
    return mock.patch.object(
        rce,
        "resolve_extension_path",
        side_effect=lambda path, base, resolve_relative: f"R:{path}",
    )


def describe_with_resolved_extensions():
    def it_passes_init_through_untouched():
        argv = ["init", "--root", "."]
        assert rce.with_resolved_extensions(argv) is argv

    def it_passes_a_native_config_through_untouched():
        argv = ["--config", "dirsql.config.py"]
        assert rce.with_resolved_extensions(argv) is argv

    def it_passes_through_when_the_config_is_missing():
        argv = ["--config", "/nope/.dirsql.toml"]
        assert rce.with_resolved_extensions(argv) is argv

    def it_passes_through_on_malformed_toml(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, "this is not = valid = toml")
        argv = ["--config", path]
        assert rce.with_resolved_extensions(argv) is argv

    def it_passes_through_when_there_is_no_dirsql_section(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*"\n')
        argv = ["--config", path]
        assert rce.with_resolved_extensions(argv) is argv

    def it_passes_through_when_no_extensions_are_declared(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[dirsql]\nignore = ["x"]\n')
        argv = ["--config", path]
        assert rce.with_resolved_extensions(argv) is argv

    def it_passes_through_when_extension_is_not_a_list(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[dirsql]\nextension = "not-a-list"\n')
        argv = ["--config", path]
        assert rce.with_resolved_extensions(argv) is argv

    def it_passes_through_when_extension_entries_are_not_tables(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, "[dirsql]\nextension = [1, 2]\n")
        argv = ["--config", path]
        assert rce.with_resolved_extensions(argv) is argv

    def it_passes_through_when_all_paths_are_literal(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[[dirsql.extension]]\npath = "ext/a.so"\n')
        argv = ["--config", path]
        with _patch() as resolver:
            assert rce.with_resolved_extensions(argv) is argv
            resolver.assert_not_called()

    def it_appends_extension_flags_for_a_bare_package_name(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(
            path,
            "[[dirsql.extension]]\n"
            'path = "sqlite_vec"\n'
            'entrypoint = "sqlite3_vec_init"\n\n'
            "[[dirsql.extension]]\n"
            'path = "ext/local.so"\n',
        )
        with _patch() as resolver:
            out = rce.with_resolved_extensions(["--config", path])
        assert out == [
            "--config",
            path,
            "--extension",
            "R:sqlite_vec::sqlite3_vec_init",
            "--extension",
            "R:ext/local.so",
        ]
        resolver.assert_any_call("sqlite_vec", base=cfg_dir, resolve_relative=True)

    def it_skips_a_non_string_path_in_the_package_name_probe(cfg_dir):
        # A dict entry whose `path` isn't a string is not treated as a package
        # name; a sibling bare name still triggers resolution.
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(
            path,
            "[[dirsql.extension]]\npath = 42\n\n"
            '[[dirsql.extension]]\npath = "sqlite_vec"\n',
        )
        with _patch():
            out = rce.with_resolved_extensions(["--config", path])
        assert out[-4:] == [
            "--extension",
            "R:42",
            "--extension",
            "R:sqlite_vec",
        ]

    def it_reads_the_config_equals_form(cfg_dir):
        path = os.path.join(cfg_dir, ".dirsql.toml")
        _write(path, '[[dirsql.extension]]\npath = "pkg"\n')
        with _patch():
            out = rce.with_resolved_extensions([f"--config={path}"])
        assert out == [f"--config={path}", "--extension", "R:pkg"]

    def it_defaults_to_dot_dirsql_toml_when_no_config_given():
        # No `--config` and no `./.dirsql.toml` in cwd -> unchanged.
        argv = ["--port", "9000"]
        assert rce.with_resolved_extensions(argv) is argv

    def it_treats_a_bare_trailing_config_as_empty_path():
        argv = ["--config"]
        assert rce.with_resolved_extensions(argv) is argv
