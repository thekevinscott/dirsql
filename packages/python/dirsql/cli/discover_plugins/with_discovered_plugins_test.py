"""Unit tests for the discovery orchestrator (all collaborators mocked)."""

from unittest.mock import patch

from . import with_discovered_plugins as module
from .with_discovered_plugins import with_discovered_plugins


def describe_with_discovered_plugins():
    def it_strips_no_plugin_and_skips_discovery():
        with (
            patch.object(module, "discovery_disabled", return_value=True),
            patch.object(module, "discovered_fragments", side_effect=AssertionError),
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
        ):
            assert with_discovered_plugins([init, "--force"]) == [init, "--force"]

    def it_leaves_context_untouched():
        context = "".join(["con", "text"])
        with (
            patch.object(module, "discovery_disabled", return_value=False),
            patch.object(module, "discovered_fragments", side_effect=AssertionError),
        ):
            assert with_discovered_plugins([context]) == [context]

    def it_is_a_no_op_when_no_plugins_are_installed():
        with (
            patch.object(module, "discovery_disabled", return_value=False),
            patch.object(module, "discovered_fragments", return_value=[]),
        ):
            assert with_discovered_plugins(["query", "x"]) == ["query", "x"]

    def it_injects_only_c_flags_for_each_plugin():
        with (
            patch.object(module, "discovery_disabled", return_value=False),
            patch.object(
                module,
                "discovered_fragments",
                return_value=["/a/dirsql.toml", "/b/dirsql.toml"],
            ),
        ):
            assert with_discovered_plugins(["query", "x"]) == [
                "query",
                "x",
                "-c",
                "/a/dirsql.toml",
                "-c",
                "/b/dirsql.toml",
            ]

    def it_appends_after_the_users_own_config():
        with (
            patch.object(module, "discovery_disabled", return_value=False),
            patch.object(
                module, "discovered_fragments", return_value=["/a/dirsql.toml"]
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
