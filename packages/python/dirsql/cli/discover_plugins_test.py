"""Unit tests for the launcher's plugin discovery (all collaborators mocked)."""

from unittest.mock import patch

import pytest

from . import discover_plugins
from .discover_plugins import (
    _discovered_fragments,
    _discovery_disabled,
    _fragment_path,
    _user_passed_config,
    with_discovered_plugins,
)


class _FakeFragment:
    def __init__(self, path: str, exists: bool):
        self._path = path
        self._exists = exists

    def is_file(self) -> bool:
        return self._exists

    def __str__(self) -> str:
        return self._path


class _FakeModuleDir:
    def __init__(self, fragment: _FakeFragment):
        self._fragment = fragment
        self.joined: str | None = None

    def joinpath(self, name: str) -> _FakeFragment:
        self.joined = name
        return self._fragment


class _FakeEP:
    def __init__(self, name: str, value: str):
        self.name = name
        self.value = value


def describe_user_passed_config():
    def it_is_true_for_the_bare_short_flag():
        assert _user_passed_config(["-c", "x.toml"]) is True

    def it_is_true_for_the_attached_short_value():
        assert _user_passed_config(["-c./x.toml"]) is True

    def it_is_true_for_the_long_flag():
        assert _user_passed_config(["--config", "x.toml"]) is True

    def it_is_true_for_the_config_equals_form():
        assert _user_passed_config(["--config=x.toml"]) is True

    def it_is_false_without_any_config_flag():
        assert _user_passed_config(["query", "SELECT 1"]) is False

    def it_does_not_mistake_a_long_lookalike_for_a_short_flag():
        # `--configx` is not `--config`, not `--config=…`, and does not start
        # with `-c` (it starts with `--`) -- so no clause matches.
        assert _user_passed_config(["--configx"]) is False

    def it_is_false_for_a_flag_that_sorts_before_config():
        # `--all` sorts lexically before `--config` but is not a config flag;
        # pins the `==` comparison against a `<=` mutant.
        assert _user_passed_config(["--all"]) is False


def describe_discovery_disabled():
    def it_is_true_with_the_no_plugin_flag():
        with patch.object(discover_plugins.os, "environ", {}):
            assert _discovery_disabled(["--no-plugin"]) is True

    def it_is_true_with_the_env_var():
        with patch.object(discover_plugins.os, "environ", {"DIRSQL_NO_PLUGIN": "1"}):
            assert _discovery_disabled([]) is True

    def it_is_false_without_either():
        with patch.object(discover_plugins.os, "environ", {}):
            assert _discovery_disabled(["query"]) is False


def describe_fragment_path():
    def it_returns_the_shipped_fragment_path():
        module_dir = _FakeModuleDir(_FakeFragment("/abs/plug/dirsql.toml", True))
        with patch.object(
            discover_plugins.resources, "files", return_value=module_dir
        ) as files:
            assert _fragment_path("plug") == "/abs/plug/dirsql.toml"
        files.assert_called_once_with("plug")
        assert module_dir.joined == "dirsql.toml"

    def it_raises_naming_the_module_when_not_importable():
        with patch.object(
            discover_plugins.resources,
            "files",
            side_effect=ModuleNotFoundError("no module named plug"),
        ):
            with pytest.raises(ValueError, match="plug"):
                _fragment_path("plug")

    def it_raises_naming_the_fragment_when_absent():
        module_dir = _FakeModuleDir(_FakeFragment("/abs/plug/dirsql.toml", False))
        with patch.object(discover_plugins.resources, "files", return_value=module_dir):
            with pytest.raises(ValueError, match="dirsql.toml"):
                _fragment_path("plug")


def describe_discovered_fragments():
    def it_orders_fragments_by_entry_point_name():
        eps = [_FakeEP("beta", "mod_b"), _FakeEP("alpha", "mod_a")]
        with (
            patch.object(
                discover_plugins.metadata, "entry_points", return_value=eps
            ) as entry_points,
            patch.object(
                discover_plugins,
                "_fragment_path",
                side_effect=lambda module: f"/{module}/dirsql.toml",
            ),
        ):
            assert _discovered_fragments() == [
                "/mod_a/dirsql.toml",
                "/mod_b/dirsql.toml",
            ]
        entry_points.assert_called_once_with(group="dirsql")


def describe_with_discovered_plugins():
    def it_strips_no_plugin_and_skips_discovery():
        with (
            patch.object(discover_plugins.os, "environ", {}),
            patch.object(
                discover_plugins, "_discovered_fragments", side_effect=AssertionError
            ),
        ):
            # `--host` sorts before `--no-plugin` and must survive the strip;
            # pins the `!=` filter against a `>` mutant that would drop it.
            assert with_discovered_plugins(
                ["--no-plugin", "--host", "h", "query", "x"]
            ) == ["--host", "h", "query", "x"]

    def it_skips_discovery_when_the_env_var_is_set():
        with (
            patch.object(discover_plugins.os, "environ", {"DIRSQL_NO_PLUGIN": "1"}),
            patch.object(
                discover_plugins, "_discovered_fragments", side_effect=AssertionError
            ),
        ):
            assert with_discovered_plugins(["query", "x"]) == ["query", "x"]

    def it_leaves_init_untouched():
        # A non-interned "init" plus a trailing arg pins the init guard against
        # both a `argv[0] is "init"` mutant (identity vs equality) and an
        # `argv[1]` index mutant -- either falls through to discovery and trips
        # the mocked guard.
        init = "".join(["i", "n", "i", "t"])
        with (
            patch.object(discover_plugins.os, "environ", {}),
            patch.object(
                discover_plugins, "_discovered_fragments", side_effect=AssertionError
            ),
        ):
            assert with_discovered_plugins([init, "--force"]) == [init, "--force"]

    def it_is_a_no_op_when_no_plugins_are_installed():
        with (
            patch.object(discover_plugins.os, "environ", {}),
            patch.object(discover_plugins, "_discovered_fragments", return_value=[]),
        ):
            assert with_discovered_plugins(["query", "x"]) == ["query", "x"]

    def it_injects_include_default_and_c_flags_without_a_user_config():
        with (
            patch.object(discover_plugins.os, "environ", {}),
            patch.object(
                discover_plugins,
                "_discovered_fragments",
                return_value=["/a/dirsql.toml", "/b/dirsql.toml"],
            ),
        ):
            # Appended after the user's args; config flags are subcommand-local
            # (#609) so they accumulate with the user's own `-c` in the same
            # clap context.
            assert with_discovered_plugins(["query", "x"]) == [
                "query",
                "x",
                "--include-default",
                "-c",
                "/a/dirsql.toml",
                "-c",
                "/b/dirsql.toml",
            ]

    def it_injects_only_c_flags_when_the_user_passed_a_config():
        with (
            patch.object(discover_plugins.os, "environ", {}),
            patch.object(
                discover_plugins,
                "_discovered_fragments",
                return_value=["/a/dirsql.toml"],
            ),
        ):
            assert with_discovered_plugins(["-c", "user.toml", "query", "x"]) == [
                "-c",
                "user.toml",
                "query",
                "x",
                "-c",
                "/a/dirsql.toml",
            ]
