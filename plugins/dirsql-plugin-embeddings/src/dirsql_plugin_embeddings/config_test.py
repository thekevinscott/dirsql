"""Colocated unit test for the plugin's cache configuration.

`CACHE_READ` is read from the environment at import, so the environment cases
re-import the module rather than patching after the fact.
"""

import importlib
import os
from datetime import timedelta
from pathlib import Path
from unittest import mock

from . import config


def _reloaded(**env):
    with mock.patch.dict(os.environ, env, clear=True):
        return importlib.reload(config)


def describe_constants():
    def it_names_the_cache_after_the_distribution():
        assert config.PLUGIN_NAME == "dirsql-plugin-embeddings"

    def it_puts_the_cache_under_the_plugin_name():
        assert config.CACHE_DIR == Path.home() / ".cache" / config.PLUGIN_NAME

    def it_keeps_entries_far_past_cachettas_default():
        # (path, mtime) keying means a hit can never be stale, so expiry would
        # only re-do work for a file nothing has touched.
        assert config.CACHE_DURATION == timedelta(days=365)
        assert config.CACHE_DURATION > timedelta(days=7)


def describe_cache_read():
    def it_reads_the_cache_by_default():
        assert _reloaded().CACHE_READ is True

    def it_stops_reading_when_the_variable_is_zero():
        assert _reloaded(**{config.ENV_CACHE_READ: "0"}).CACHE_READ is False

    def it_treats_any_other_value_as_enabled():
        assert _reloaded(**{config.ENV_CACHE_READ: "1"}).CACHE_READ is True


def teardown_module():
    # `_reloaded` mutated the imported module object; restore it so import
    # order cannot leak a patched environment into another test file.
    importlib.reload(config)
