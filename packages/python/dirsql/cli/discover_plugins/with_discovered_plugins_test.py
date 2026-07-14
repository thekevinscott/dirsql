"""Unit tests for the discovery orchestrator (all collaborators mocked)."""

from unittest.mock import patch

from . import with_discovered_plugins as module
from .with_discovered_plugins import with_discovered_plugins


def describe_with_discovered_plugins():
    def it_strips_no_plugin_and_skips_discovery():
        with (
            patch.object(module, "discovery_disabled", return_value=True),
            patch.object(module, "discovered_fragments", side_effect=AssertionError),
            patch.object(module, "user_passed_config", side_effect=AssertionError),
        ):
            # `--host` sorts before `--no-plugin` and must survive the strip;
            # pins the `!=` filter against a `>` mutant that would drop it.
            assert with_discovered_plugins(
                ["--no-plugin", "--host", "h", "query", "x"]
            ) == ["--host", "h", "query", "x"]

    def it_leaves_init_untouched():
        # A non-interned "init" plus a trailing arg pins the init guard against
        # both a `argv[0] is "init"` mutant (identity vs equality) and an
        # `argv[1]` index mutant -- either falls through to discovery and trips
        # the mocked guard.
        init = "".join(["i", "n", "i", "t"])
        with (
            patch.object(module, "discovery_disabled", return_value=False),
            patch.object(module, "discovered_fragments", side_effect=AssertionError),
            patch.object(module, "user_passed_config", side_effect=AssertionError),
        ):
            assert with_discovered_plugins([init, "--force"]) == [init, "--force"]

    def it_is_a_no_op_when_no_plugins_are_installed():
        with (
            patch.object(module, "discovery_disabled", return_value=False),
            patch.object(module, "discovered_fragments", return_value=[]),
            patch.object(module, "user_passed_config", side_effect=AssertionError),
        ):
            assert with_discovered_plugins(["query", "x"]) == ["query", "x"]

    def it_injects_include_default_and_c_flags_without_a_user_config():
        with (
            patch.object(module, "discovery_disabled", return_value=False),
            patch.object(
                module,
                "discovered_fragments",
                return_value=["/a/dirsql.toml", "/b/dirsql.toml"],
            ),
            patch.object(module, "user_passed_config", return_value=False),
        ):
            # Appended after the user's args; config flags are subcommand-local
            # (#609) so they accumulate with the user's own `-c`.
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
            patch.object(module, "discovery_disabled", return_value=False),
            patch.object(
                module, "discovered_fragments", return_value=["/a/dirsql.toml"]
            ),
            patch.object(module, "user_passed_config", return_value=True),
        ):
            assert with_discovered_plugins(["-c", "user.toml", "query", "x"]) == [
                "-c",
                "user.toml",
                "query",
                "x",
                "-c",
                "/a/dirsql.toml",
            ]
