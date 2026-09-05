"""Unit tests for `_load_toml_module`.

`import_module` is mocked and `sys.version_info` patched so both arms run on
every interpreter -- neither `tomllib` (absent on 3.10) nor `tomli` (absent on
3.11+) is importable everywhere.
"""

from unittest import mock

import dirsql.toml_module as mod


def _requested(version):
    with (
        mock.patch.object(mod.sys, "version_info", version),
        mock.patch.object(mod, "import_module") as import_module,
    ):
        assert mod._load_toml_module() is import_module.return_value
    (name,) = import_module.call_args.args
    return name


def describe_load_toml_module():
    # Exactly `(3, 11)`, not `(3, 11, 0)`: the latter compares greater than the
    # bare `(3, 11)` literal, so it cannot tell `>=` from `>`.
    def it_uses_the_stdlib_parser_on_the_version_that_gained_it():
        assert _requested((3, 11)) == "tomllib"

    def it_uses_the_stdlib_parser_on_newer_versions():
        assert _requested((3, 12, 4)) == "tomllib"

    def it_uses_the_tomli_backport_below_311():
        assert _requested((3, 10, 17)) == "tomli"


def describe_module_binding():
    def it_binds_the_parser_the_running_interpreter_asked_for():
        assert mod._toml.__name__ == (
            "tomllib" if mod.sys.version_info >= (3, 11) else "tomli"
        )
