"""Colocated unit test for the plugin's cache configuration."""

import os
from datetime import timedelta
from pathlib import Path
from unittest import mock

from . import config


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


def describe_cache_reads_enabled():
    def it_reads_the_cache_by_default():
        with mock.patch.dict(os.environ, {}, clear=True):
            assert config.cache_reads_enabled() is True

    def it_stops_reading_when_the_variable_is_zero():
        with mock.patch.dict(os.environ, {config.ENV_CACHE_READ: "0"}):
            assert config.cache_reads_enabled() is False

    def it_treats_any_other_value_as_enabled():
        with mock.patch.dict(os.environ, {config.ENV_CACHE_READ: "1"}):
            assert config.cache_reads_enabled() is True
