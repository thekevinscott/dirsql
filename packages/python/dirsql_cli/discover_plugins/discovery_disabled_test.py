"""Unit tests for `discovery_disabled` (env mocked)."""

from unittest.mock import patch

from . import discovery_disabled as module
from .discovery_disabled import discovery_disabled


def describe_discovery_disabled():
    def it_is_true_with_the_no_plugin_flag():
        with patch.object(module.os, "environ", {}):
            assert discovery_disabled(["--no-plugin"]) is True

    def it_is_true_with_the_env_var():
        with patch.object(module.os, "environ", {"DIRSQL_NO_PLUGIN": "1"}):
            assert discovery_disabled([]) is True

    def it_is_false_without_either():
        with patch.object(module.os, "environ", {}):
            assert discovery_disabled(["query"]) is False
